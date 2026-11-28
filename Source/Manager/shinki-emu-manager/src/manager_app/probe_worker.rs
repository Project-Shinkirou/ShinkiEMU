use super::*;
use shinki_emu_manager_core::detection::DetectionError;

const SCAN_PROGRESS_INTERVAL: usize = 64;

pub(super) fn canonical_directory(path: &Path) -> Result<Option<PathBuf>, String> {
    let canonical = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error)
            if matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(format!("SHINKI_EMU_GAME_DIRECTORY_IO: {error}")),
    };
    match fs::metadata(&canonical) {
        Ok(metadata) => Ok(metadata.is_dir().then_some(canonical)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("SHINKI_EMU_GAME_DIRECTORY_IO: {error}")),
    }
}

fn confirmed_missing_local_directory(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        use windows::Win32::{
            Storage::FileSystem::GetDriveTypeW, System::WindowsProgramming::DRIVE_FIXED,
        };
        use windows::core::PCWSTR;

        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return false;
        };
        let drive = match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => return false,
        };
        let root = [u16::from(drive), u16::from(b':'), u16::from(b'\\'), 0];
        // SAFETY: the drive root is a live, nul-terminated UTF-16 string.
        if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != DRIVE_FIXED {
            return false;
        }
    }
    // A broken link still exists, and a lost parent cannot prove that its
    // child was deleted rather than temporarily disconnected.
    matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        && path.parent().is_some_and(|parent| fs::read_dir(parent).is_ok())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScanKind {
    Full,
    Import,
}

pub(super) struct ScanRequest {
    pub(super) kind: ScanKind,
    pub(super) offer_unrecognized: bool,
    pub(super) paths: Vec<PathBuf>,
    pub(super) games: Vec<GameRecord>,
    pub(super) excluded: BTreeSet<PathBuf>,
    pub(super) cancel: shinki_emu_manager_core::NativeCancellation,
    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
    pub(super) optical: Option<OpticalVolume>,
}

pub(super) struct GameProbe {
    pub(super) path: PathBuf,
    pub(super) game_ids: Vec<String>,
    pub(super) detections: Vec<shinki_emu_manager_core::detection::EngineDetection>,
    pub(super) patch_profile: Option<Result<shinki_emu_patch::OutputProfile, String>>,
    #[cfg(target_os = "android")]
    pub(super) saf: Option<SafProbe>,
}

impl GameProbe {
    pub(super) fn matches_output(&self, output: &Path) -> bool {
        if self.path == output { return true; }
        #[cfg(target_os = "android")]
        if self.saf.as_ref().and_then(|saf| saf.local_path.as_deref()) == Some(output) {
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UnrecognizedRoot {
    pub(super) requested: PathBuf,
    pub(super) path: PathBuf,
    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
    pub(super) optical: Option<OpticalVolume>,
    #[cfg(target_os = "android")]
    pub(super) saf: Option<SafProbe>,
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SafProbe {
    pub(super) title: String,
    pub(super) local_path: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct ScanReport {
    pub(super) games: Vec<GameProbe>,
    pub(super) unrecognized_root: Option<UnrecognizedRoot>,
    pub(super) missing_local_games: BTreeSet<String>,
    pub(super) skipped_io: usize,
    pub(super) cache_hits: usize,
    pub(super) cache_error: Option<String>,
    pub(super) skipped_candidates: usize,
    pub(super) traversal_limited: bool,
    #[cfg(target_os = "android")]
    pub(super) saf_error: Option<String>,
}

impl ScanReport {
    pub(super) fn has_io_error(&self) -> bool {
        #[cfg(target_os = "android")]
        if self.saf_error.is_some() {
            return true;
        }
        self.skipped_io != 0
    }

    fn record_traversal_limit(&mut self, scope: &str) {
        tracing::info!(event = "shinki.emu.manager.library_scan_traversal_limited", scope);
        self.traversal_limited = true;
    }
}

pub(super) fn scan(
    cache: &mut probe_cache::ProbeCache,
    request: ScanRequest,
    shutdown: &std::sync::atomic::AtomicBool,
    progress: &mut dyn FnMut(super::owner::PrepareProgress),
) -> Result<ScanReport, String> {
    let cancelled = || {
        if request.cancel.is_cancelled() {
            Some("SHINKI_EMU_SCAN_CANCELLED")
        } else if shutdown.load(std::sync::atomic::Ordering::Acquire) {
            Some("SHINKI_EMU_OWNER_CANCELLED")
        } else {
            None
        }
    };
    if let Some(error) = cancelled() {
        return Err(error.into());
    }
    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
    if let Some(volume) = &request.optical {
        volume.validate()?;
    }
    let excluded: BTreeSet<PathBuf> = request
        .excluded
        .iter()
        .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect();
    let mut report = ScanReport::default();
    #[cfg(target_os = "android")]
    scan_saf(cache.rules(), &request, &mut report, shutdown, &cancelled, progress)?;
    let mut known = BTreeMap::<PathBuf, Vec<String>>::new();
    for game in &request.games {
        if let Some(error) = cancelled() {
            return Err(error.into());
        }
        #[cfg(target_os = "android")]
        if shinki_emu_manager::platform::android_tree::is_library_uri(&game.location) {
            continue;
        }
        let location = Path::new(&game.location);
        match canonical_directory(location) {
            Ok(Some(path)) if excluded.contains(&path) => continue,
            Ok(Some(path)) if !shinki_emu_patch::is_staging_path(&path) => {
                known.entry(path).or_default().push(game.game_id.clone())
            }
            Ok(Some(_)) => continue,
            Ok(None) => {
                report.skipped_io += 1;
                if confirmed_missing_local_directory(location) {
                    report.missing_local_games.insert(game.game_id.clone());
                }
            }
            Err(_) => report.skipped_io += 1,
        }
    }
    let mut roots = BTreeSet::new();
    let mut selected = None;
    if request.kind == ScanKind::Full {
        roots.extend(known.keys().cloned());
        cache.retain_registered(&request.games);
    }
    for root in request.paths {
        #[cfg(target_os = "android")]
        if root.to_str().is_some_and(shinki_emu_manager::platform::android_tree::is_library_uri) {
            continue;
        }
        let path = match canonical_directory(&root) {
            Ok(Some(path)) if shinki_emu_patch::is_staging_path(&path) => {
                if request.kind == ScanKind::Full {
                    continue;
                }
                return Err(
                    "SHINKI_EMU_PATCH_INVALID_REQUEST: private staging cannot be imported".into(),
                );
            }
            Ok(Some(path)) => path,
            Ok(None) | Err(_) => {
                report.skipped_io += 1;
                continue;
            }
        };
        if request.offer_unrecognized && !known.contains_key(&path) && !excluded.contains(&path) {
            selected = Some(UnrecognizedRoot {
                requested: root, path: path.clone(),
                #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
                optical: request.optical.clone(),
                #[cfg(target_os = "android")]
                saf: None,
            });
        }
        roots.insert(path);
    }
    let mut visited = BTreeSet::new();
    let mut recognized = Vec::<PathBuf>::new();
    let mut saw_file = false;
    for root in roots {
        let mut queue = vec![(root, 0_usize)];
        while let Some((path, depth)) = queue.pop() {
            if let Some(error) = cancelled() {
                return Err(error.into());
            }
            if excluded.contains(&path)
                || (!known.contains_key(&path)
                    && recognized.iter().any(|game| path.starts_with(game)))
                || visited.contains(&path)
            {
                continue;
            }
            if visited.len() == MAX_SCAN_DIRECTORIES {
                report.record_traversal_limit("local_tree");
                break;
            }
            visited.insert(path.clone());
            if visited.len() % SCAN_PROGRESS_INTERVAL == 0 {
                progress(super::owner::PrepareProgress::Scan {
                    directories: visited.len(),
                    games: recognized.len(),
                });
            }
            let allow_hint = request.kind == ScanKind::Full && known.contains_key(&path);
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            let detection = if request.optical.is_some() {
                // The ordinary probe cache is path-keyed; a drive path can
                // identify entirely different game data after an eject.
                cache.rules().detect_game_directory(&path, &request.cancel).map(|detections| (detections, false))
            } else {
                cache.detect(&path, allow_hint, &request.cancel)
            };
            #[cfg(not(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos")))))]
            let detection = cache.detect(&path, allow_hint, &request.cancel);
            let detected = match detection {
                Ok((detections, hit)) => {
                    report.cache_hits += usize::from(hit);
                    Some(detections)
                }
                Err(DetectionError::ReadLimit) => {
                    report.skipped_candidates += 1;
                    None
                }
                Err(DetectionError::Cancelled) => return Err("SHINKI_EMU_SCAN_CANCELLED".into()),
                Err(DetectionError::Io(_)) => {
                    report.skipped_io += 1;
                    None
                }
            };
            let examined = detected.is_some();
            let detections = detected.unwrap_or_default();
            let matched = !detections.is_empty();
            // An import interrupted after saving its root is discovered on the
            // next startup. Validate the receipt before registering a game.
            let patch_profile = if examined && (request.kind == ScanKind::Import || (matched && !known.contains_key(&path))) {
                match shinki_emu_patch::read_output_profile(&path, shutdown) {
                    Ok(Some(profile)) => Some(Ok(profile)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error.to_string())),
                }
            } else {
                None
            };
            let has_receipt = patch_profile.is_some();
            if matched || (examined && known.contains_key(&path)) || has_receipt {
                report.games.push(GameProbe {
                    path: path.clone(),
                    game_ids: known.get(&path).cloned().unwrap_or_default(),
                    detections,
                    patch_profile,
                    #[cfg(target_os = "android")]
                    saf: None,
                });
            }
            if matched || has_receipt {
                recognized.push(path);
            } else {
                let mut children = Vec::new();
                let Ok(entries) = fs::read_dir(&path) else {
                    report.skipped_io += 1;
                    continue;
                };
                for entry in entries {
                    let Ok(entry) = entry else {
                        report.skipped_io += 1;
                        continue;
                    };
                    let Ok(kind) = entry.file_type() else {
                        report.skipped_io += 1;
                        continue;
                    };
                    if selected.is_some() && kind.is_file() {
                        saw_file = true;
                    }
                    let child = entry.path();
                    if kind.is_dir()
                        && !kind.is_symlink()
                        && !shinki_emu_patch::is_staging_path(&child)
                    {
                        if depth == MAX_SCAN_DEPTH {
                            report.record_traversal_limit("local_depth");
                            break;
                        }
                        if children.len() + queue.len() + visited.len() == MAX_SCAN_DIRECTORIES {
                            report.record_traversal_limit("local_children");
                            break;
                        }
                        children.push(child);
                    }
                }
                children.sort();
                queue.extend(children.into_iter().rev().map(|path| (path, depth + 1)));
            }
        }
    }
    if saw_file {
        report.unrecognized_root = selected.filter(|root| visited.contains(&root.path));
    }
    if let Some(error) = cancelled() {
        return Err(error.into());
    }
    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
    if let Some(volume) = &request.optical {
        volume.validate()?;
        return Ok(report);
    }
    report.cache_error = cache.save().err();
    Ok(report)
}

#[cfg(target_os = "android")]
fn scan_saf(
    rules: &shinki_emu_manager_core::detection::DeclaredRules,
    request: &ScanRequest,
    report: &mut ScanReport,
    shutdown: &std::sync::atomic::AtomicBool,
    cancelled: &dyn Fn() -> Option<&'static str>,
    progress: &mut dyn FnMut(super::owner::PrepareProgress),
) -> Result<(), String> {
    use shinki_emu_manager::platform::android_tree as saf;

    fn key(uri: &str) -> (String, String) {
        (saf::authority(uri).to_owned(), saf::document_identity(uri).to_owned())
    }
    fn report_failure(report: &mut ScanReport, error: DetectionError, scope: &str) -> Result<(), String> {
        match error {
            DetectionError::Cancelled => return Err("SHINKI_EMU_SCAN_CANCELLED".into()),
            DetectionError::ReadLimit if scope == "candidate" => report.skipped_candidates += 1,
            DetectionError::ReadLimit => report.record_traversal_limit(scope),
            DetectionError::Io(error) => {
                report.skipped_io += 1;
                report.saf_error.get_or_insert_with(|| format!("SHINKI_EMU_GAME_DIRECTORY_IO: {error}"));
            }
        }
        Ok(())
    }

    let known: BTreeMap<_, _> = request.games.iter().filter(|game| saf::is_library_uri(&game.location))
        .map(|game| (key(&game.location), game)).collect();
    let excluded: BTreeSet<_> = request.excluded.iter().filter_map(|path| path.to_str())
        .filter(|uri| saf::is_library_uri(uri)).map(key).collect();
    let roots = request.paths.iter().filter_map(|path| path.to_str()).filter(|uri| saf::is_library_uri(uri));
    let saved = known.values().map(|game| (game.location.as_str(), true))
        .filter(|_| request.kind == ScanKind::Full);
    let mut visited = BTreeSet::new();
    let mut attempted = 0;
    for (uri, saved_only) in roots.map(|uri| (uri, false)).chain(saved) {
        if let Some(error) = cancelled() { return Err(error.into()); }
        if saved_only && visited.contains(&key(uri)) { continue; }
        let root = match saf::document(uri) {
            Ok(root) if root.directory => root,
            Ok(_) => {
                report_failure(report, std::io::Error::new(std::io::ErrorKind::InvalidInput, "selected document is not a folder").into(), "saf_root")?;
                continue;
            }
            Err(error) => {
                report_failure(report, error, "saf_root")?;
                continue;
            }
        };
        let mut selected = None;
        let mut saw_file = false;
        let mut pending = vec![(root, 0)];
        while let Some((dir, depth)) = pending.pop() {
            if let Some(error) = cancelled() { return Err(error.into()); }
            if shinki_emu_patch::is_staging_path(Path::new(&dir.name)) { continue; }
            let identity = key(&dir.uri);
            if excluded.contains(&identity) || visited.contains(&identity) { continue; }
            if attempted == MAX_SCAN_DIRECTORIES {
                report.record_traversal_limit("saf_tree");
                return Ok(());
            }
            attempted += 1;
            if attempted % SCAN_PROGRESS_INTERVAL == 0 {
                progress(super::owner::PrepareProgress::Scan { directories: attempted, games: report.games.len() });
            }
            // The app must be able to open game resources, not just list a trusted volume.
            let local = saf::local_path(&dir.uri).ok().flatten().and_then(|path| {
                let (matches, has_files) = rules.inspect_game_directory(&path, &request.cancel).ok()?;
                Some((matches, has_files, path))
            });
            let (detected, has_files, local_path) = match local {
                Some((matches, has_files, path)) if !matches.is_empty() => (Some(matches), has_files, Some(path)),
                unrecognized_local => match rules.inspect_game_tree(&saf::SafTree::new(&dir), &request.cancel) {
                    Ok((matches, has_files)) => {
                        // A provider-only match does not prove direct access to those files.
                        let local_path = unrecognized_local.filter(|_| matches.is_empty()).map(|(_, _, path)| path);
                        (Some(matches), has_files, local_path)
                    }
                    Err(error) => {
                        report_failure(report, error, "candidate")?;
                        // A failed candidate need not hide independently readable child games.
                        (None, false, None)
                    }
                },
            };
            if let Some(error) = cancelled() { return Err(error.into()); }
            let game_known = known.get(&identity).copied();
            let matched = detected.as_ref().is_some_and(|detected| !detected.is_empty());
            if detected.is_some() {
                visited.insert(identity);
            }
            if request.offer_unrecognized {
                saw_file |= has_files;
                if depth == 0 && detected.is_some() && !matched && game_known.is_none() {
                    let no_patch = local_path.as_deref().is_none_or(|path| {
                        matches!(shinki_emu_patch::read_output_profile(path, shutdown), Ok(None))
                    });
                    if no_patch {
                        selected = Some(UnrecognizedRoot {
                            requested: PathBuf::from(uri),
                            path: PathBuf::from(&dir.uri),
                            saf: Some(SafProbe { title: dir.name.clone(), local_path: local_path.clone() }),
                        });
                    }
                }
            }
            if !matched && !saved_only {
                let limit = if depth == MAX_SCAN_DEPTH { 0 } else { MAX_SCAN_DIRECTORIES - attempted - pending.len() };
                match saf::child_directories(&dir.uri, limit) {
                    Ok(mut children) => {
                        children.sort_unstable_by(|left, right| left.name.cmp(&right.name));
                        pending.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
                    }
                    Err(DetectionError::ReadLimit) if depth == MAX_SCAN_DEPTH => report.record_traversal_limit("saf_depth"),
                    Err(error) => report_failure(report, error, "saf_children")?,
                }
            }
            if matched || (detected.is_some() && game_known.is_some()) {
                let detections = detected.unwrap_or_default();
                let game_ids = game_known.map(|game| vec![game.game_id.clone()]).unwrap_or_default();
                let patch_profile = if matched && (request.kind == ScanKind::Import || game_known.is_none()) {
                    match local_path.as_deref() {
                        Some(path) => shinki_emu_patch::read_output_profile(path, shutdown)
                            .transpose().map(|profile| profile.map_err(|error| error.to_string())),
                        None => None,
                    }
                } else {
                    None
                };
                report.games.push(GameProbe {
                    path: PathBuf::from(dir.uri),
                    game_ids,
                    detections,
                    patch_profile,
                    saf: Some(SafProbe { title: dir.name, local_path }),
                });
            }
        }
        if saw_file && selected.is_some() {
            report.unrecognized_root = selected;
        }
    }
    Ok(())
}
