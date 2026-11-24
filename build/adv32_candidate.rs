//! Local Manager embedding is independent of the distributable static roster.
pub fn linked(core: &toml::Value, target: &str, enabled: bool) -> bool {
    enabled
        && core.get("id").and_then(toml::Value::as_str) == Some("shinki.emu.adv32")
        && core
            .get("static_manager_candidate_targets")
            .and_then(toml::Value::as_array)
            .is_some_and(|targets| targets.iter().any(|value| value.as_str() == Some(target)))
}
