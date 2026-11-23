use std::collections::BTreeSet;
use std::fmt::Write;

#[path = "../../../../build/android_dynamic.rs"]
mod android_dynamic;

/// Build-time projection of the authoritative inventories. No core manifests or
/// source checkouts are opened, and publication restrictions do not affect use.
pub fn generate(cores: &str, targets: &str, target: &str) -> Result<String, String> {
    let cores: toml::Value = toml::from_str(cores).map_err(|error| format!("core inventory: {error}"))?;
    let targets: toml::Value = toml::from_str(targets).map_err(|error| format!("target inventory: {error}"))?;
    if text(&cores, "schema")? != "shinki.emu.build_cores.v2"
        || text(&targets, "schema")? != "shinki.emu.build_targets.v1"
    {
        return Err("unsupported build inventory schema".into());
    }
    let declared = array(&targets, "targets")?
        .iter()
        .find(|row| row.get("triple").and_then(toml::Value::as_str) == Some(target));
    let enabled = declared.is_some_and(|row| {
        row.get("blocked").and_then(toml::Value::as_str).unwrap_or_default().is_empty()
    });
    let mut ids = BTreeSet::new();
    let mut output = String::from("static ENTRIES: &[CoreCatalogEntry] = &[\n");
    for core in array(&cores, "cores")? {
        let plugin_id = text(core, "id")?;
        let family_id = text(core, "family_id")?;
        let display_name = text(core, "display_name")?;
        if plugin_id.is_empty()
            || plugin_id.len() > 128
            || !plugin_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            || !ids.insert(plugin_id)
        {
            return Err(format!("invalid or duplicate plugin id: {plugin_id}"));
        }
        if family_id.is_empty()
            || family_id.len() > 64
            || !family_id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !family_id.as_bytes()[0].is_ascii_lowercase()
            || display_name.trim().is_empty()
        {
            return Err(format!("invalid family_id or display_name for {plugin_id}"));
        }
        let embedded = enabled && contains_target(core, "static_manager_targets", target)?;
        let standalone = enabled && contains_target(core, "supported_targets", target)?;
        let supported = embedded || standalone;
        let first_party = core.get("attribution").is_none();
        let policy = android_dynamic::parse(core)?;
        let android_dynamic = if standalone && android_dynamic::TARGETS.contains(&target) {
            policy.map(|(minimum, requires_context)| {
                let minimum = match minimum {
                    android_dynamic::MinimumRelease::Sequence(sequence) =>
                        format!("AndroidMinimumRelease::Sequence({sequence})"),
                    android_dynamic::MinimumRelease::Semver(version) =>
                        format!("AndroidMinimumRelease::Semver({version:?})"),
                };
                format!("Some(AndroidDynamicPolicy {{ minimum: {minimum}, requires_context: {requires_context} }})")
            })
        } else {
            None
        };
        let android_dynamic = android_dynamic.as_deref().unwrap_or("None");
        let embedded_recognition_json = if embedded {
            core.get("recognition").map(serde_json::to_string).transpose()
                .map_err(|error| format!("recognition for {plugin_id}: {error}"))?
        } else {
            None
        };
        let recognition = embedded_recognition_json.as_deref().unwrap_or("[]");
        writeln!(output,
            "    CoreCatalogEntry {{ plugin_id: {plugin_id:?}, family_id: {family_id:?}, display_name: {display_name:?}, first_party: {first_party}, supported: {supported}, standalone: {standalone}, android_dynamic: {android_dynamic}, embedded: {embedded}, embedded_recognition_json: {recognition:?} }},"
        ).expect("String write");
    }
    if ids.is_empty() {
        return Err("empty core inventory".into());
    }
    output.push_str("];\n");
    Ok(output)
}

fn text<'a>(value: &'a toml::Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(toml::Value::as_str).ok_or_else(|| format!("missing string field {key}"))
}

fn array<'a>(value: &'a toml::Value, key: &str) -> Result<&'a Vec<toml::Value>, String> {
    value.get(key).and_then(toml::Value::as_array).ok_or_else(|| format!("missing array field {key}"))
}

fn contains_target(value: &toml::Value, key: &str, target: &str) -> Result<bool, String> {
    let Some(values) = value.get(key) else {
        return if key == "static_manager_targets" { Ok(false) } else { Err(format!("missing array field {key}")) };
    };
    let values = values.as_array().ok_or_else(|| format!("invalid target list {key}"))?;
    let mut matched = false;
    for value in values {
        let value = value.as_str().ok_or_else(|| format!("non-string target in {key}"))?;
        matched |= value == target;
    }
    Ok(matched)
}
