//! Static engine relationships and display hints from `build/cores.toml`.
//!
//! Reading this catalog performs no filesystem access, registration, library
//! loading or native calls. Package identity still comes from a validated
//! installed manifest; an unknown plugin ID has no catalog entry.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCatalogEntry {
    pub plugin_id: &'static str,
    pub family_id: &'static str,
    pub display_name: &'static str,
    /// Inventory-declared availability for the current target, through either
    /// a standalone core or static Manager integration. This does not mean
    /// installed, loaded or verified with a real game. Publication restrictions
    /// do not change runtime support.
    pub supported: bool,
    /// A standalone artifact is declared for this exact target, independently
    /// of whether the Manager also links this core.
    pub standalone: bool,
    /// Validated dynamic-release policy for the current Android ABI, if approved.
    pub android_dynamic: Option<AndroidDynamicPolicy>,
    /// Inventory-declared static Manager integration for the current target.
    /// Actual compiled entrypoints come from the host's `STATIC_CORES` table,
    /// including optional integrations such as desktop `static-cmvs`.
    /// Reading either catalog metadata or function-table IDs invokes no core.
    pub embedded: bool,
    /// The recognition field of this target's embedded core recipe, or an empty list.
    pub embedded_recognition_json: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidDynamicPolicy {
    pub minimum: AndroidMinimumRelease,
    pub requires_context: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidMinimumRelease {
    Sequence(u64),
    Semver(&'static str),
}

include!(concat!(env!("OUT_DIR"), "/static-core-catalog.rs"));

/// Every declared implementation, including those unsupported on this target.
pub fn entries() -> &'static [CoreCatalogEntry] {
    ENTRIES
}

/// Exact, case-sensitive package plugin identity; unknown IDs remain unknown.
pub fn lookup(plugin_id: &str) -> Option<&'static CoreCatalogEntry> {
    ENTRIES.iter().find(|entry| entry.plugin_id == plugin_id)
}
