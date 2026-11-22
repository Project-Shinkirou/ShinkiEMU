use super::owner::{
    BackgroundEvent, BackgroundSender, Command, Commands, PrepareProgress, Update, collect_metadata,
};
use super::*;
use crossbeam_channel::RecvTimeoutError;
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn run(
    data_dir: PathBuf,
    mailbox: FrameMailbox,
    commands: Commands,
    shared: Arc<Mutex<Update>>,
    results: BackgroundSender,
    wake: Arc<Mutex<Option<HostWake>>>,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut registry = FamilyProviderRegistry::new();
    shinki_emu_manager::startup::mark("core_loading_started");
    let initialized = (|| {
        if cancelled.load(Ordering::Acquire) {
            return Err("SHINKI_EMU_OWNER_CANCELLED".into());
        }
        let cores = data_dir.join("cores");
        fs::create_dir_all(&cores).map_err(|_| "SHINKI_EMU_CORES_DIRECTORY_CREATE")?;
        // App startup settled the previous session's operations. Also apply
        // packages staged in this session before the first lazy core load.
        let pending_failures = core_package::apply_pending(&cores);
        let directories = recognition::directories(&data_dir)?;
        let mut errors = BTreeMap::new();
        let installed = recognition::installed(&directories)?;
        for (file, error) in pending_failures {
            errors.insert(file, error);
        }
        Ok((installed, errors))
    })();
    let installed = match initialized {
        Ok((installed, errors)) => {
            let _ = results.send(BackgroundEvent::Ready(Ok((installed.clone(), errors))));
            notify(&wake);
            installed
        }
        Err(error) => {
            let _ = results.send(BackgroundEvent::Ready(Err(error)));
            notify(&wake);
            return registry.shutdown().map_err(|error| error.to_string());
        }
    };
    let mut cache = probe_cache::ProbeCache::load(&data_dir, recognition::scan_rules(&installed));
    let mut active: Option<ActiveFamilySession> = None;
    let mut pending = Vec::new();
    loop {
        if cancelled.load(Ordering::Acquire) {
            let primary = shared.lock().error.take();
            let session_error = active.take().and_then(|mut session| session.close().err());
            let owner_error = registry.shutdown().err().map(|error| error.to_string());
            let errors: Vec<_> = [primary, session_error, owner_error]
                .into_iter()
                .flatten()
                .collect();
            return if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("; "))
            };
        }
        let command = if let Some(session) = active.as_ref() {
            commands.recv_timeout(
                session
                    .wake_deadline()
                    .saturating_duration_since(Instant::now()),
            )
        } else {
            commands.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match command {
            #[cfg(test)]
            Ok(Command::Registry(call)) => {
                let (plugin_id, result) = call(&mut registry);
                let _ = results.send(BackgroundEvent::Activated { plugin_id, result });
                notify(&wake);
            }
            Ok(Command::Scan(request)) => {
                let kind = request.kind;
                let result = if active.is_some() {
                    Err("SHINKI_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into())
                } else {
                    probe_worker::scan(
                        &mut cache,
                        request,
                        &cancelled,
                        &mut |progress| {
                            let _ = results.send(BackgroundEvent::Progress(progress));
                            notify(&wake);
                        },
                    )
                };
                let _ = results.send(BackgroundEvent::Scanned(kind, result));
                notify(&wake);
            }
            Ok(Command::Activate(plugin_id)) => {
                let result = if active.is_some() {
                    Err("SHINKI_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into())
                } else {
                    activate(&mut registry, &installed, &plugin_id)
                        .and_then(|()| collect_metadata(&registry, &plugin_id))
                };
                let _ = results.send(BackgroundEvent::Activated { plugin_id, result });
                notify(&wake);
            }
            Ok(Command::Open(request)) => {
                pending.clear();
                let epoch = request.epoch;
                let candidate = (|| {
                    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
                    if let Some(volume) = &request.optical {
                        volume.validate()?;
                    }
                    activate(&mut registry, &installed, &request.candidate.plugin_id)?;
                    let path = fs::canonicalize(&request.game.location).map_err(|error| {
                        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
                        if let Some(volume) = &request.optical
                            && let Err(error) = volume.validate()
                        {
                            return error;
                        }
                        error.to_string()
                    })?;
                    let selection = registry.probe_selected(
                        &ProbeRequest { game_path: path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?.into() },
                        &request.candidate.plugin_id,
                    ).map_err(|error| error.to_string());
                    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
                    if let Some(volume) = &request.optical {
                        volume.validate()?;
                    }
                    match selection? {
                        FamilyProbeSelection::Selected(candidate) if candidate.report.family_id == request.candidate.family_id => Ok(candidate),
                        _ => Err("SHINKI_EMU_FAMILY_FRESH_PROBE_NO_MATCH".to_owned()),
                    }
                })();
                let candidate = match candidate {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        if let Some(text) = &request.text { let _ = text.close(); }
                        if let Some(window) = &request.window_service { window.close(); }
                        if let Some(modal) = &request.modal { modal.cancel_pending(); }
                        request.cancellation.cancel();
                        closed(epoch, Some(error), &shared);
                        notify(&wake);
                        continue;
                    }
                };
                match ActiveFamilySession::open(
                    (&request.game, request.configuration, epoch),
                    &candidate,
                    &mut registry,
                    mailbox.clone(),
                    request.window,
                    request.render_extent,
                    super::session::SessionServices {
                        text: request.text,
                        window: request.window_service,
                        modal: request.modal,
                        cancellation: request.cancellation,
                    },
                ) {
                    Ok(mut session) => {
                        publish(&mut session, &shared);
                        active = Some(session);
                    }
                    Err(error) => closed(epoch, Some(error), &shared),
                }
                notify(&wake);
            }
            Ok(Command::Events { epoch, events }) => {
                if let Some(session) = active.as_mut().filter(|session| session.epoch() == epoch) {
                    if pending.len() + events.len()
                        > shinki_emu_manager_core::model::MAX_INPUT_EVENTS
                    {
                        tracing::warn!(
                            event = "shinki.emu.host.input_dropped",
                            diagnostic_code = "SHINKI_EMU_INPUT_QUEUE_FULL",
                            dropped = events.len()
                        );
                    } else {
                        pending.extend(events);
                        let result = session.input(&pending);
                        pending.clear();
                        if let Err(error) = result {
                            finish(&mut active, Some(error), &shared);
                        }
                        notify(&wake);
                    }
                }
            }
            Ok(Command::Action { epoch, control }) => {
                if let Some(session) = active.as_mut().filter(|session| session.epoch() == epoch)
                    && let Err(error) = session.control(control)
                {
                    if session.poisoned() {
                        finish(&mut active, Some(error), &shared);
                    } else {
                        let mut update = shared.lock();
                        if update.epoch == epoch {
                            update.action_error = Some(error);
                            update.dirty = true;
                        }
                    }
                    notify(&wake);
                }
            }
            Ok(Command::Close { epoch, events }) => {
                if let Some(session) = active.as_mut().filter(|session| session.epoch() == epoch) {
                    let result = if pending.is_empty() {
                        Ok(())
                    } else {
                        session.input(&pending)
                    };
                    pending.clear();
                    let result = result.and_then(|()| {
                        if events.is_empty() {
                            Ok(())
                        } else {
                            session.input(&events)
                        }
                    });
                    finish(&mut active, result.err(), &shared);
                    notify(&wake);
                }
            }
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                let primary = shared.lock().error.take();
                let session_error = active.take().and_then(|mut session| session.close().err());
                let owner_error = registry.shutdown().err().map(|error| error.to_string());
                let errors: Vec<_> = [primary, session_error, owner_error]
                    .into_iter()
                    .flatten()
                    .collect();
                return if errors.is_empty() {
                    Ok(())
                } else {
                    Err(errors.join("; "))
                };
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if active
            .as_ref()
            .is_some_and(|session| session.status == FamilyStatus::Finished)
        {
            finish(&mut active, None, &shared);
            pending.clear();
            notify(&wake);
        }
        // Polling observes native production; it never drives VM scheduling.
        if let Some(session) = active.as_mut() {
            let result = service_due(session, &mut pending, Instant::now());
            let finished = session.status == FamilyStatus::Finished;
            match result {
                Err(error) => {
                    pending.clear();
                    finish(&mut active, Some(error), &shared);
                    notify(&wake);
                }
                Ok(_) if finished => {
                    finish(&mut active, None, &shared);
                    notify(&wake);
                }
                Ok(true) => {
                    if let Some(session) = active.as_mut() {
                        publish(session, &shared);
                    }
                    notify(&wake);
                }
                Ok(false) => {}
            }
        }
    }
}

pub(super) fn service_due(
    session: &mut ActiveFamilySession,
    pending: &mut Vec<FamilyEvent>,
    now: Instant,
) -> Result<bool, String> {
    if !pending.is_empty() {
        session.input(pending)?;
        pending.clear();
        if session.status == FamilyStatus::Finished {
            return Ok(false);
        }
    }
    if now >= session.wake_deadline() {
        session.poll()
    } else {
        Ok(false)
    }
}

use shinki_emu_manager::static_cores::STATIC_CORES;

fn activate(
    registry: &mut FamilyProviderRegistry,
    installed: &BTreeMap<String, recognition::InstalledCore>,
    plugin_id: &str,
) -> Result<(), String> {
    // Synthetic providers can only enter through the test-only Registry command.
    #[cfg(test)]
    if registry.descriptor(plugin_id).is_some() { return Ok(()); }
    let catalog = recognition::runnable_core(installed, plugin_id)?;
    if registry.descriptor(plugin_id).is_some() { return Ok(()); }
    #[cfg(target_os = "android")]
    {
        let root = crate::platform::bundled_resource_dir()
            .ok_or("SHINKI_EMU_DATA_DIRECTORY_UNAVAILABLE")?;
        match plugin_id {
            "renpy" => {
                let resources = root.join("cores/renpy/Resources");
                let path = resources.to_str().ok_or("RENPY_RESOURCE_ROOT_ENCODING")?;
                let mut error = shinki_emu_abi::Error::default();
                let status = shinki_emu_renpy::set_resource_root(path, &mut error);
                if status != shinki_emu_abi::OK { return Err(format!("RENPY_RESOURCE_ROOT ({status})")); }
            }
            "kirikiroid2-web" | "krkr2" => {
                let resources = root.join(format!("cores/{plugin_id}/Resources"));
                let path = resources.to_str().ok_or("KRKR_RESOURCE_ROOT_ENCODING")?;
                let status = if plugin_id == "krkr2" { shinki_emu_krkr2::set_resource_root(path) } else { shinki_emu_kirikiroid2_web::set_resource_root(path) };
                if status != shinki_emu_abi::OK { return Err(format!("KRKR_RESOURCE_ROOT ({status})")); }
            }
            _ => {}
        }
    }
    let provider = if let Some((_, query)) = STATIC_CORES.iter().find(|(id, _)| *id == plugin_id) {
        unsafe { shinki_emu_manager_core::LoadedFamilyPlugin::from_query(*query) }.map_err(|error| error.to_string())?
    } else {
        let core = &installed[plugin_id];
        shinki_emu_manager_core::LoadedFamilyPlugin::load(&core.library).map_err(|error| error.to_string())?
    };
    let descriptor = provider.manager_descriptor();
    if descriptor.plugin_id != plugin_id || catalog.family_id != descriptor.family_id {
        return Err("SHINKI_EMU_CORE_PACKAGE_PLUGIN_MISMATCH".into());
    }
    registry.register_provider(provider).map_err(|error| error.to_string())?;
    Ok(())
}

fn publish(session: &mut ActiveFamilySession, shared: &Mutex<Update>) {
    let mut update = shared.lock();
    if update.epoch != session.epoch() {
        return;
    }
    if !update.opened {
        update.opened = true;
        update.surface = Some(session.surface);
        update.dirty = true;
    }
    if update.failure != session.failure {
        update.failure.clone_from(&session.failure);
        update.dirty = true;
    }
}

fn finish(
    active: &mut Option<ActiveFamilySession>,
    primary: Option<String>,
    shared: &Mutex<Update>,
) {
    if let Some(mut session) = active.take() {
        let epoch = session.epoch();
        let finished = session.status == FamilyStatus::Finished;
        let cleanup = session.close().err();
        let error = match (primary, cleanup) {
            (Some(first), Some(second)) => Some(format!("{first}; {second}")),
            (first, second) => first.or(second),
        };
        let mut update = shared.lock();
        if update.epoch == epoch {
            update.finished = finished;
        }
        drop(update);
        closed(epoch, error, shared);
    }
}

fn closed(epoch: u64, error: Option<String>, shared: &Mutex<Update>) {
    let mut update = shared.lock();
    if update.epoch != epoch {
        return;
    }
    update.closed = true;
    update.error = error;
    update.dirty = true;
}

fn notify(wake: &Mutex<Option<HostWake>>) {
    let callback = wake.lock().clone();
    if let Some(wake) = callback {
        wake();
    }
}
