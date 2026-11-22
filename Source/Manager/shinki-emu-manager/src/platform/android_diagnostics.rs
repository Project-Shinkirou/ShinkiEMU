//! Export only stable fields from the active on-device Roukaku log.
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

const LOG_SCHEMA: &str = "roukaku.log_event.v1";
const MAX_BYTES: u64 = 512 * 1024;
const MAX_EVENTS: usize = 200;

// These are fixed Manager events, not prefixes accepted from native cores.
const EVENTS: &[&str] = &[
    "shinki.emu.android.library_tree_scan_failed",
    "shinki.emu.android.pick_result_dropped",
    "shinki.emu.android.pick_result_unknown",
    "shinki.emu.android.startup_failed",
    "shinki.emu.android.storage_access_unavailable",
    "shinki.emu.core.close_failed",
    "shinki.emu.host.game_terminated",
    "shinki.emu.host.renderer_failed",
    "shinki.emu.library.rebuilt",
    "shinki.emu.manager.core_download_failed",
    "shinki.emu.manager.core_downloaded",
    "shinki.emu.manager.core_pending_failed",
    "shinki.emu.manager.fatal",
    "shinki.emu.manager.launch.failed",
    "shinki.emu.manager.launch.opening",
    "shinki.emu.manager.launch.requested",
    "shinki.emu.manager.library_scan_failed",
    "shinki.emu.manager.library_scan_traversal_limited",
    "shinki.emu.metadata.sync_failed",
    "shinki.emu.native.call_failed",
    "shinki.emu.native.session_failed",
    "shinki.emu.play.session_ended",
    "shinki.emu.play.session_started",
    "shinki.emu.startup.phase",
];

const DIAGNOSTIC_CODES: &[&str] = &[
    "SHINKI_EMU_ANDROID_PICK_EVENT_LOOP",
    "SHINKI_EMU_ANDROID_PICK_PERMISSION",
    "SHINKI_EMU_ANDROID_PICK_REQUEST",
    "SHINKI_EMU_ANDROID_PICK_RESULT",
    "SHINKI_EMU_ANDROID_PICK_TYPE",
    "SHINKI_EMU_ANDROID_PICK_UNKNOWN",
    "SHINKI_EMU_ANDROID_STORAGE_SETTINGS",
    "SHINKI_EMU_GAME_DIRECTORY_BOUNDS",
    "SHINKI_EMU_GAME_DIRECTORY_ENUMERATION",
    "SHINKI_EMU_GAME_DIRECTORY_INVALID",
    "SHINKI_EMU_GAME_DIRECTORY_IO",
    "SHINKI_EMU_GAME_PATH_UTF8",
    "SHINKI_EMU_HOST_RENDERER",
    "SHINKI_EMU_LIBRARY_INVALID_LOCATION",
    "SHINKI_EMU_SCAN_CANCELLED",
];

#[derive(Deserialize)]
struct Record<'a> {
    schema: &'a str,
    timestamp: &'a str,
    level: &'a str,
    event: &'a str,
    #[serde(borrow, default)]
    fields: Codes<'a>,
}

#[derive(Default, Deserialize)]
struct Codes<'a> {
    #[serde(borrow)]
    diagnostic_code: Option<&'a str>,
    #[serde(borrow)]
    code: Option<&'a str>,
}

#[derive(Serialize)]
struct SummaryEvent<'a> {
    timestamp: &'a str,
    level: &'a str,
    event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_code: Option<&'a str>,
}

pub(super) fn summary(data_dir: &Path) -> io::Result<Option<Vec<u8>>> {
    let path = data_dir.join("diagnostics").join("roukaku.jsonl");
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "android")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active diagnostic log is not a file",
        ));
    }
    let end = metadata.len();
    let start = end.saturating_sub(MAX_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::with_capacity((end - start) as usize);
    file.take(end - start).read_to_end(&mut tail)?;
    let tail = if start == 0 {
        tail.as_slice()
    } else if let Some(first) = tail.iter().position(|&byte| byte == b'\n') {
        &tail[first + 1..]
    } else {
        return Ok(None);
    };
    let Some(last_newline) = tail.iter().rposition(|&byte| byte == b'\n') else {
        return Ok(None);
    };
    let events: Vec<_> = tail[..last_newline]
        .rsplit(|&byte| byte == b'\n')
        .filter_map(summary_event)
        .take(MAX_EVENTS)
        .collect();
    if events.is_empty() {
        return Ok(None);
    }
    let mut output = Vec::new();
    for event in events.iter().rev() {
        serde_json::to_writer(&mut output, event).map_err(io::Error::other)?;
        output.push(b'\n');
    }
    Ok(Some(output))
}

fn summary_event(line: &[u8]) -> Option<SummaryEvent<'_>> {
    let record = serde_json::from_slice::<Record<'_>>(line).ok()?;
    if record.schema != LOG_SCHEMA
        || !utc_timestamp(record.timestamp)
        || !matches!(record.level, "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR")
        || !EVENTS.contains(&record.event)
    {
        return None;
    }
    let diagnostic_code = [record.fields.diagnostic_code, record.fields.code]
        .into_iter()
        .flatten()
        .map(|value| value.split_once(':').map_or(value, |(code, _)| code))
        .find(|code| DIAGNOSTIC_CODES.contains(code));
    Some(SummaryEvent {
        timestamp: record.timestamp,
        level: record.level,
        event: record.event,
        diagnostic_code,
    })
}

// Roukaku writes UTC RFC 3339 timestamps, with an optional fractional second.
fn utc_timestamp(timestamp: &str) -> bool {
    let Some(utc) = timestamp.strip_suffix('Z') else {
        return false;
    };
    let date = if let Some((date, fraction)) = utc.split_once('.') {
        if !(1..=9).contains(&fraction.len()) || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        date
    } else {
        utc
    };
    date.len() == 19 && date.bytes().enumerate().all(|(index, byte)| match index {
        4 | 7 => byte == b'-',
        10 => byte == b'T',
        13 | 16 => byte == b':',
        _ => byte.is_ascii_digit(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::fs;

    #[test]
    fn shares_only_recognized_events_and_approved_fields_from_the_active_log() {
        let data = tempfile::tempdir().unwrap();
        let diagnostics = data.path().join("diagnostics");
        fs::create_dir(&diagnostics).unwrap();
        fs::write(diagnostics.join("roukaku-critical.jsonl"), b"private-critical").unwrap();
        fs::write(diagnostics.join("roukaku.jsonl.1"), b"private-archive").unwrap();
        assert!(summary(data.path()).unwrap().is_none());
        fs::write(diagnostics.join("roukaku.jsonl"), b"").unwrap();
        assert!(summary(data.path()).unwrap().is_none());
        let stable = json!({
            "schema": LOG_SCHEMA, "timestamp": "2026-10-10T10:10:10.1Z", "level": "WARN",
            "event": "shinki.emu.manager.launch.failed", "target": "private-target", "session_id": "private-session",
            "fields": { "diagnostic_code": "SHINKI_EMU_HOST_RENDERER: private-detail", "message": "private-message", "token": "private-token", "path": "private-path" },
            "span_stack": [{"name": "private-span"}]
        });
        let mut native = stable.clone();
        native["event"] = json!("shinki.emu.private-native-event");
        native["fields"]["diagnostic_code"] = json!("SHINKI_EMU_PRIVATE_NATIVE_CODE");
        let mut untrusted_code = stable.clone();
        untrusted_code["fields"]["diagnostic_code"] = native["fields"]["diagnostic_code"].clone();
        fs::write(diagnostics.join("roukaku.jsonl"), format!("{native}\n")).unwrap();
        assert!(summary(data.path()).unwrap().is_none());
        fs::write(
            diagnostics.join("roukaku.jsonl"),
            format!("{stable}\n{{broken}}\n{native}\n{untrusted_code}\n{stable}"),
        ).unwrap();
        let export = summary(data.path()).unwrap().unwrap();
        let records = serde_json::Deserializer::from_slice(&export).into_iter::<Value>()
            .collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(records, [
            json!({ "timestamp": "2026-10-10T10:10:10.1Z", "level": "WARN", "event": "shinki.emu.manager.launch.failed", "diagnostic_code": "SHINKI_EMU_HOST_RENDERER" }),
            json!({ "timestamp": "2026-10-10T10:10:10.1Z", "level": "WARN", "event": "shinki.emu.manager.launch.failed" }),
        ]);
    }

    #[test]
    fn truncates_to_the_latest_complete_records_in_the_bounded_tail() {
        let data = tempfile::tempdir().unwrap();
        let diagnostics = data.path().join("diagnostics");
        fs::create_dir(&diagnostics).unwrap();
        let path = diagnostics.join("roukaku.jsonl");
        let line = |event| json!({ "schema": LOG_SCHEMA, "timestamp": "2026-10-10T10:10:10Z", "level": "INFO", "event": event });
        let old = line("shinki.emu.startup.phase");
        let first = line("shinki.emu.manager.launch.requested");
        let last = line("shinki.emu.manager.launch.failed");
        let mut log = format!("{old}\n{}\n", "X".repeat(MAX_BYTES as usize));
        for _ in 0..MAX_EVENTS {
            log.push_str(&format!("{first}\n"));
        }
        log.push_str(&format!("{last}\n{old}"));
        fs::write(path, log).unwrap();
        let out = summary(data.path()).unwrap().unwrap();
        let text = std::str::from_utf8(&out).unwrap();
        assert_eq!(text.lines().count(), MAX_EVENTS);
        assert!(!text.contains("startup.phase"));
        assert!(text.lines().last().unwrap().contains("launch.failed"));
    }
}
