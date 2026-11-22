//! Shared by xtask inventory validation and the Manager's generated catalog.

pub const TARGETS: [&str; 2] = ["aarch64-linux-android", "x86_64-linux-android"];

pub enum MinimumRelease<'a> {
    Sequence(u64),
    Semver(&'a str),
}

pub fn parse(core: &toml::Value) -> Result<Option<(MinimumRelease<'_>, bool)>, String> {
    let Some(policy) = core.get("android_dynamic") else {
        return Ok(None);
    };
    let id = core.get("id").and_then(toml::Value::as_str).unwrap_or("?");
    let invalid = |reason: &str| format!("{id}: android_dynamic {reason}");
    let table = policy.as_table().ok_or_else(|| invalid("must be a table"))?;
    if table.len() != 2 || !table.contains_key("minimum_release") || !table.contains_key("requires_context") {
        return Err(invalid("requires only minimum_release and requires_context"));
    }
    let supported = core.get("supported_targets").and_then(toml::Value::as_array);
    if !supported.is_some_and(|targets| {
        targets.iter().any(|target| target.as_str().is_some_and(|target| TARGETS.contains(&target)))
    }) {
        return Err(invalid("requires a supported Android standalone target"));
    }
    let minimum = &table["minimum_release"];
    let floor = if let Some(source) = core.get("source") {
        let current = source.get("release_sequence").and_then(toml::Value::as_integer);
        let sequence = minimum.as_integer().filter(|&sequence| {
            (1..=9_007_199_254_740_991).contains(&sequence)
                && current.is_some_and(|current| sequence <= current)
        }).ok_or_else(|| invalid("requires a positive release sequence at or below the source pin"))?;
        MinimumRelease::Sequence(sequence as u64)
    } else {
        let version = minimum.as_str().filter(|version| {
            *version != "0.0.0-local" && version.len() <= 64 && semver::Version::parse(version).is_ok()
        }).ok_or_else(|| invalid("requires a release SemVer for an in-repo core"))?;
        MinimumRelease::Semver(version)
    };
    let requires_context = table["requires_context"].as_bool()
        .ok_or_else(|| invalid("requires_context must be a boolean"))?;
    Ok(Some((floor, requires_context)))
}
