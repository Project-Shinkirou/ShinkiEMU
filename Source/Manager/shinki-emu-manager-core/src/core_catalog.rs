//! Engine relationships and target availability generated from `build/cores.toml`.
//!
//! Reading this catalog performs no filesystem access, registration, library
//! loading or native calls. Package identity still comes from a validated
//! installed manifest; an unknown plugin ID has no catalog entry.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCatalogEntry {
    pub plugin_id: &'static str,
    pub family_id: &'static str,
    pub display_name: &'static str,
    /// In-repo cores share the product release identity, not an adapter sequence.
    pub first_party: bool,
    /// The inventory declares a standalone artifact for this target.
    /// This does not mean the core is installed, loaded or game-compatible.
    /// Publication restrictions do not change runtime support.
    pub supported: bool,
    /// Validated dynamic-release policy for the current Android ABI, if approved.
    pub android_dynamic: Option<AndroidDynamicPolicy>,
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

include!(concat!(env!("OUT_DIR"), "/core-catalog.rs"));

/// Every declared implementation, including those unsupported on this target.
pub fn entries() -> &'static [CoreCatalogEntry] {
    ENTRIES
}

/// Exact, case-sensitive package plugin identity; unknown IDs remain unknown.
pub fn lookup(plugin_id: &str) -> Option<&'static CoreCatalogEntry> {
    ENTRIES.iter().find(|entry| entry.plugin_id == plugin_id)
}
