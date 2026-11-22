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

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
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
