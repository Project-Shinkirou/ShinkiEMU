//! The single inventory of Manager-embedded query entrypoints, shared by UI and headless hosts.
pub const STATIC_CORES: &[(&str, shinki_emu_abi::Query)] =
    include!(concat!(env!("OUT_DIR"), "/static-core-queries.rs"));
