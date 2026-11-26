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
        // Desktop also accepts stages created before its first lazy load.
        // Android applies stages only when a new app process starts.
        #[cfg(not(target_os = "android"))]
        let errors = {
            let cores = core_directory(&data_dir)?;
            fs::create_dir_all(&cores).map_err(|_| "SHINKI_EMU_CORES_DIRECTORY_CREATE")?;
            core_package::apply_pending(&cores).into_iter().collect()
        };
        #[cfg(target_os = "android")]
        let errors = startup_core_package_failures().iter().cloned().collect();
        let directories = recognition::directories(&data_dir)?;
        let installed = recognition::installed(&directories)?;
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
            Ok(Command::ValidateConfigPath {
                plugin_id,
                field_id,
                path,
                game,
                game_relative,
                reply,
            }) => {
                if !reply.is_closed() {
                    let result = if active.is_some() {
                        Err("SHINKI_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into())
                    } else {
                        (|| {
                            let path = family_config::canonical_config_path(&path)?;
                            registry
                                .validate_config_path(&plugin_id, &field_id, &path)
                                .map_err(|error| error.to_string())?;
                            family_config::config_path_value(&path, &game, game_relative)
                        })()
                    };
                    if reply.send(result).is_ok() {
                        notify(&wake);
                    }
                }
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

fn activate(
    registry: &mut FamilyProviderRegistry,
    installed: &BTreeMap<String, recognition::InstalledCore>,
    plugin_id: &str,
) -> Result<(), String> {
    // Synthetic providers can only enter through the test-only Registry command.
    #[cfg(test)]
    if registry.descriptor(plugin_id).is_some() { return Ok(()); }
    let catalog = recognition::supported_core(plugin_id)?;
    if registry.descriptor(plugin_id).is_some() { return Ok(()); }
    let provider = match recognition::core_source(installed, plugin_id).ok_or("SHINKI_EMU_CORE_MISSING")? {
        #[cfg(any(feature = "static-cmvs", feature = "static-adv32"))]
        recognition::CoreSource::Static(query) => unsafe {
            shinki_emu_manager_core::LoadedFamilyPlugin::from_query(query)
        },
        recognition::CoreSource::Package(core) => {
            #[cfg(target_os = "android")]
            {
                let policy = catalog.android_dynamic.ok_or("SHINKI_EMU_CORE_UNSUPPORTED_TARGET")?;
                if !android_core_library(&core.library) {
                    return Err("SHINKI_EMU_CORE_UNSUPPORTED_TARGET".into());
                }
                // Package dependencies use Bionic's DT_RUNPATH ($ORIGIN).
                // Packaged cores locate their own resources beside the library.
                let (vm, application) = crate::platform::android::core_application_context()?;
                // SAFETY: the host holds these handles for the process lifetime.
                unsafe {
                    shinki_emu_manager_core::LoadedFamilyPlugin::load_android(
                        &core.library, vm, application, policy.requires_context,
                    )
                }
            }
            #[cfg(all(not(target_os = "android"), not(target_env = "ohos")))]
            {
                // iOS code signing controls executable mappings. Bind this path
                // to the exact binary recorded by the app seal before dlopen.
                #[cfg(target_os = "ios")]
                crate::core_package::verify_bundled_executable(&core.manifest, &core.library)?;
                shinki_emu_manager_core::LoadedFamilyPlugin::load(&core.library)
            }
            #[cfg(target_env = "ohos")]
            {
                if !core.bundled {
                    return Err("SHINKI_EMU_CORE_UNSUPPORTED_TARGET".into());
                }
                Ok(crate::platform::ohos::bundled::load(plugin_id)?)
            }
        }
    }.map_err(|error| error.to_string())?;
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
