//! Package naming and adapter identities shared by build-time consumers.

pub fn adapter_id(id: &str) -> &str {
    id.strip_prefix("shinki.emu.").unwrap_or(id)
}

#[derive(Clone, Copy)]
pub struct LockedAdapter<'a> {
    pub repo: &'a str,
    pub commit: &'a str,
}

/// Read the resolved commit (the fragment), not the requested Git selector.
/// A local Cargo patch has no Git source to publish.
pub fn locked_adapter<'a>(lock: &'a toml::Value, id: &str) -> Option<LockedAdapter<'a>> {
    let id = adapter_id(id);
    let mut entries = lock.get("package")?.as_array()?.iter().filter(|package| {
        package.get("name").and_then(toml::Value::as_str)
            .and_then(|name| name.strip_prefix("shinki-emu-")) == Some(id)
    });
    let source = entries.next()?.get("source")?.as_str()?.strip_prefix("git+")?;
    assert!(entries.next().is_none(), "multiple locked adapter packages for {id}");
    let (url, commit) = source.rsplit_once('#')?;
    let repo = url.split('?').next()?;
    Some(LockedAdapter { repo: repo.trim_end_matches(".git"), commit })
}
