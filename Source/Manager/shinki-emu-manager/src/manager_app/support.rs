use super::*;
use shinki_emu_manager_ui_slint::tr;
use time::{OffsetDateTime, PlainDateTime};

pub(super) const NETWORK_ALLOWED_KEY: &str = "network.allowed";
pub(super) const TRANSLATION_ENABLED_KEY: &str = "translation.enabled";
pub(super) const SENSITIVE_COVERS_KEY: &str = "covers.sensitive";
pub(super) const ONBOARDING_COMPLETED_KEY: &str = "onboarding.completed";
pub(super) const BANGUMI_SECRET_REFERENCE: &str = "metadata.bangumi";

pub(super) fn unix_time_ms() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "SHINKI_EMU_SYSTEM_CLOCK_INVALID".to_owned())?
        .as_millis()
        .try_into()
        .map_err(|_| "SHINKI_EMU_SYSTEM_CLOCK_OVERFLOW".to_owned())
}

pub(super) fn platform_data_dir() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("SHINKI_EMU_DATA_DIR") {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err("SHINKI_EMU_DATA_DIRECTORY_INVALID".into());
        }
        return Ok(path);
    }
    shinki_emu_manager::platform::default_data_dir()
}

pub(super) fn core_directory(data_dir: &Path) -> Result<PathBuf, String> {
    #[cfg(target_os = "android")]
    {
        // SHINKI_EMU_DATA_DIR can relocate the library database, never native code.
        let _ = data_dir;
        let root = shinki_emu_manager::platform::default_data_dir()?
            .canonicalize()
            .map_err(|error| format!("SHINKI_EMU_CORES_DIRECTORY_CREATE: {error}"))?
            .join("cores");
        for directory in [&root, &root.join(core_package::PENDING)] {
            match fs::symlink_metadata(directory) {
                Ok(metadata) if metadata.is_dir() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("SHINKI_EMU_CORE_DIRECTORY: invalid private core directory".into()),
            }
        }
        Ok(root)
    }
    #[cfg(not(target_os = "android"))]
    Ok(data_dir.join("cores"))
}

#[cfg(target_os = "android")]
static ANDROID_STARTUP_CORE_PACKAGES: std::sync::OnceLock<Vec<(String, String)>> =
    std::sync::OnceLock::new();

pub(super) fn apply_startup_core_packages(cores: &Path) -> Vec<(String, String)> {
    #[cfg(target_os = "android")]
    {
        // NativeActivity can recreate the controller in this process while
        // LoadedFamilyPlugin deliberately keeps the mapped libraries resident.
        apply_core_packages_once(&ANDROID_STARTUP_CORE_PACKAGES, cores).to_vec()
    }
    #[cfg(not(target_os = "android"))]
    core_package::apply_pending(cores)
}

#[cfg(target_os = "android")]
pub(super) fn startup_core_package_failures() -> &'static [(String, String)] {
    ANDROID_STARTUP_CORE_PACKAGES.get().map_or(&[], Vec::as_slice)
}

#[cfg(any(target_os = "android", test))]
fn apply_core_packages_once<'a>(
    applied: &'a std::sync::OnceLock<Vec<(String, String)>>,
    cores: &Path,
) -> &'a [(String, String)] {
    applied.get_or_init(|| core_package::apply_pending(cores))
}

/// Filter foreign or truncated installed files before calling libdl. Android's
/// package builder verifies the full ELF, SONAME and dependency closure.
#[cfg(any(target_os = "android", test))]
fn android_shared_object_header(header: &[u8; 64], machine: u16) -> bool {
    header[..7] == *b"\x7fELF\x02\x01\x01"
        && u16::from_le_bytes([header[16], header[17]]) == 3 // ET_DYN
        && u16::from_le_bytes([header[18], header[19]]) == machine
        && u32::from_le_bytes(header[20..24].try_into().unwrap()) == 1
}

#[cfg(target_os = "android")]
pub(super) fn android_core_library(path: &Path) -> bool {
    use std::io::Read;

    let machine = match std::env::consts::ARCH {
        "aarch64" => 183, // EM_AARCH64
        "x86_64" => 62,   // EM_X86_64
        _ => return false,
    };
    let mut header = [0; 64];
    path.extension().is_some_and(|ext| ext == "so")
        && fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
        && fs::File::open(path).and_then(|mut file| file.read_exact(&mut header)).is_ok()
        && android_shared_object_header(&header, machine)
}

#[cfg(test)]
mod android_core_tests {
    use super::{android_shared_object_header, apply_core_packages_once, core_package, fs};
    use std::sync::OnceLock;

    #[test]
    fn staged_core_update_waits_for_a_new_process() {
        let root = tempfile::tempdir().unwrap();
        let cores = root.path();
        let stem = "shinki_emu_siglus_rs";
        let current = cores.join(stem);
        fs::create_dir(&current).unwrap();
        fs::write(current.join("version"), b"old").unwrap();
        let process = OnceLock::new();
        assert!(apply_core_packages_once(&process, cores).is_empty());

        let staged = cores.join(core_package::PENDING).join(stem);
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("version"), b"new").unwrap();
        assert!(apply_core_packages_once(&process, cores).is_empty());
        assert_eq!(fs::read(current.join("version")).unwrap(), b"old");
        assert!(staged.is_dir());

        assert!(apply_core_packages_once(&OnceLock::new(), cores).is_empty());
        assert_eq!(fs::read(current.join("version")).unwrap(), b"new");
        assert!(!staged.exists());
    }

    #[test]
    fn android_package_header_must_match_the_process_abi() {
        let mut header = [0; 64];
        header[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        header[16..18].copy_from_slice(&3u16.to_le_bytes());
        header[18..20].copy_from_slice(&183u16.to_le_bytes());
        header[20..24].copy_from_slice(&1u32.to_le_bytes());
        assert!(android_shared_object_header(&header, 183));
        assert!(!android_shared_object_header(&header, 62));
        header[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert!(android_shared_object_header(&header, 62));
        assert!(!android_shared_object_header(&header, 183));
        header[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        assert!(!android_shared_object_header(&header, 62));
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
pub(super) fn core_directories(user_cores: &Path) -> Result<Vec<PathBuf>, String> {
    let bundled = shinki_emu_manager::platform::bundled_resource_dir()
        .ok_or("SHINKI_EMU_EXECUTABLE_LOCATION")?
        .join("cores");
    let mut directories = vec![user_cores.to_owned()];
    if bundled.is_dir() {
        directories.push(bundled);
    }
    Ok(directories)
}

pub(super) fn sha256_id(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn cover_extension(media_type: &str) -> Result<&'static str, String> {
    match media_type {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        "image/gif" => Ok("gif"),
        "image/bmp" => Ok("bmp"),
        _ => Err("SHINKI_EMU_METADATA_COVER_MEDIA_TYPE".into()),
    }
}

pub(super) fn store_cover(
    data_dir: &Path,
    game_id: &str,
    cover: &CoverAsset,
) -> Result<(), String> {
    let directory = data_dir.join("covers");
    fs::create_dir_all(&directory).map_err(|_| "SHINKI_EMU_METADATA_COVER_DIRECTORY")?;
    let extension = cover_extension(&cover.media_type)?;
    for previous in ["png", "jpg", "webp", "gif", "bmp"] {
        let path = directory.join(format!("{game_id}.{previous}"));
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("SHINKI_EMU_METADATA_COVER_REMOVE".into()),
        }
    }
    fs::write(
        directory.join(format!("{game_id}.{extension}")),
        &cover.bytes,
    )
    .map_err(|_| "SHINKI_EMU_METADATA_COVER_WRITE".into())
}

pub(super) fn path_id(path: &Path) -> String {
    format!("game-{}", sha256_id(&path.to_string_lossy()))
}

pub(super) fn title_for_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Untitled game")
        .to_owned()
}

pub(super) fn parse_game_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for name in ["SHINKI_EMU_GAME_DIR", "SHINKI_EMU_GAME_DIRS"] {
        if let Some(value) = env::var_os(name) {
            roots.extend(env::split_paths(&value));
        }
    }
    roots
}

pub(super) fn human_duration(ms: i64) -> String {
    let seconds = (ms.max(0) / 1000) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
}
pub(super) fn human_relative(timestamp: i64, now: i64) -> String {
    let seconds = ((now.saturating_sub(timestamp)) / 1000).max(0);
    if seconds >= 86_400 {
        tr!("{n} 天前", n = seconds / 86_400)
    } else if seconds >= 3600 {
        tr!("{n} 小时前", n = seconds / 3600)
    } else if seconds >= 60 {
        tr!("{n} 分钟前", n = seconds / 60)
    } else {
        tr!("刚刚").into()
    }
}

pub(super) fn local_time_label(unix_ms: i64) -> String {
    let Ok(utc) = OffsetDateTime::from_unix_timestamp(unix_ms.div_euclid(1_000)) else {
        return String::new();
    };
    match system_local_time(utc) {
        Some(local) => history_time_label(local, ""),
        None => history_time_label(PlainDateTime::new(utc.date(), utc.time()), " UTC"),
    }
}

fn history_time_label(datetime: PlainDateTime, suffix: &str) -> String {
    let (year, month, day) = datetime.date().to_calendar_date();
    let month = u8::from(month);
    let (hour, minute, _) = datetime.time().as_hms();
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}{suffix}")
}

#[cfg(not(windows))]
fn system_local_time(utc: OffsetDateTime) -> Option<PlainDateTime> {
    // Bionic's localtime_r reads the device timezone and applies its rules
    // at this timestamp. The locked time crate uses it directly on Android.
    let local = utc.checked_to_offset(time::UtcOffset::local_offset_at(utc).ok()?)?;
    Some(PlainDateTime::new(local.date(), local.time()))
}

#[cfg(windows)]
fn system_local_time(utc: OffsetDateTime) -> Option<PlainDateTime> {
    use windows::Win32::System::Time::{GetDynamicTimeZoneInformation, TIME_ZONE_ID_INVALID};

    let mut zone = Default::default();
    // SAFETY: The API writes to the supplied timezone structure.
    if unsafe { GetDynamicTimeZoneInformation(&mut zone) } == TIME_ZONE_ID_INVALID {
        return None;
    }
    windows_local_time(utc, &zone)
}

#[cfg(windows)]
fn windows_local_time(
    utc: OffsetDateTime,
    zone: &windows::Win32::System::Time::DYNAMIC_TIME_ZONE_INFORMATION,
) -> Option<PlainDateTime> {
    use windows::Win32::{Foundation::SYSTEMTIME, System::Time::SystemTimeToTzSpecificLocalTimeEx};

    let (year, month, day) = utc.date().to_calendar_date();
    if year < 1601 {
        return None;
    }
    let input = SYSTEMTIME {
        wYear: year.try_into().ok()?,
        wMonth: u8::from(month).into(),
        wDay: day.into(),
        wHour: utc.hour().into(),
        wMinute: utc.minute().into(),
        wSecond: utc.second().into(),
        ..Default::default()
    };
    let mut local = SYSTEMTIME::default();
    // SAFETY: All structures are valid for the call. Read the result only on success.
    // The dynamic API uses the rules in effect for the recorded instant.
    unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(zone), &input, &mut local) }.ok()?;
    let date = time::Date::from_calendar_date(
        local.wYear.into(),
        u8::try_from(local.wMonth).ok()?.try_into().ok()?,
        local.wDay.try_into().ok()?,
    )
    .ok()?;
    date.with_hms(
        local.wHour.try_into().ok()?,
        local.wMinute.try_into().ok()?,
        local.wSecond.try_into().ok()?,
    )
    .ok()
}

#[cfg(all(
    test,
    any(
        windows,
        all(unix, not(any(target_os = "android", target_os = "ios", target_env = "ohos")))
    )
))]
mod tests {
    use super::*;

    // These dates have the same UTC time, but different Pacific offsets.
    const WINTER: i64 = 1_705_284_000_000; // 2024-01-15 02:00 UTC
    const SUMMER: i64 = 1_721_008_800_000; // 2024-07-15 02:00 UTC

    fn assert_history(winter: &str, summer: &str) {
        let root = tempfile::tempdir().unwrap();
        let mut app =
            ShinkiEmuManagerController::open_library(root.path().into(), FrameMailbox::new(), Vec::new())
                .unwrap();
        app.library
            .add_game(&GameRecord {
                game_id: "history".into(),
                title: "History".into(),
                user_title: None,
                location: "history-game".into(),
                family_id: None,
                content_fingerprint: None,
                added_at_unix_ms: WINTER,
            })
            .unwrap();
        for start in [WINTER, SUMMER] {
            let id = app.library.start_play_session("history", start).unwrap();
            app.library
                .end_play_session(&id, start + 60_000, PlaySessionEndReason::Leave)
                .unwrap();
        }
        let model = app.select_case("history").unwrap();
        let rows = &model.selection.history;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].started, summer);
        assert_eq!(rows[1].started, winter);
        assert!(rows.iter().all(|row| row.duration == "1m"));
    }

    #[cfg(windows)]
    #[test]
    fn play_history_time_uses_windows_daylight_rules_and_labels_utc_fallback() {
        use windows::Win32::System::Time::EnumDynamicTimeZoneInformation;

        let mut zone = windows::Win32::System::Time::DYNAMIC_TIME_ZONE_INFORMATION::default();
        let key: Vec<u16> = "Pacific Standard Time".encode_utf16().chain([0]).collect();
        for index in 0.. {
            // SAFETY: The API writes to the supplied timezone structure.
            assert_eq!(unsafe { EnumDynamicTimeZoneInformation(index, &mut zone) }, 0);
            if zone.TimeZoneKeyName.starts_with(&key) {
                break;
            }
        }
        for (ms, expected) in [
            (WINTER, "2024-01-14 18:00"),
            (SUMMER, "2024-07-14 19:00"),
        ] {
            let utc = OffsetDateTime::from_unix_timestamp(ms / 1_000).unwrap();
            assert_eq!(
                history_time_label(windows_local_time(utc, &zone).unwrap(), ""),
                expected
            );
        }
        // Exercise the real Manager rows with the Windows system timezone.
        let [winter, summer] = [WINTER, SUMMER].map(local_time_label);
        assert_eq!(winter.len(), 16, "{winter}");
        assert_eq!(summer.len(), 16, "{summer}");
        assert_history(&winter, &summer);
        assert_eq!(local_time_label(-62_135_596_800_000), "0001-01-01 00:00 UTC");
    }

    #[cfg(all(unix, not(any(target_os = "android", target_os = "ios", target_env = "ohos"))))]
    #[test]
    fn play_history_time_uses_the_child_process_timezone() {
        const CHILD: &str = "SHINKI_TEST_PLAY_HISTORY_ZONE";
        if env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(env::current_exe().unwrap())
                .args(["--exact", "manager_app::support::tests::play_history_time_uses_the_child_process_timezone"])
                .env(CHILD, "1")
                .env("TZ", "PST8PDT,M3.2.0,M11.1.0")
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            return;
        }

        assert_history("2024-01-14 18:00", "2024-07-14 19:00");
    }
}
