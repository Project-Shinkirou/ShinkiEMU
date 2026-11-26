//! Query entrypoints for deprecated local compatibility builds.
#[cfg(any(feature = "static-cmvs", feature = "static-adv32"))]
pub const STATIC_CORES: &[(&str, shinki_emu_abi::Query)] =
    include!(concat!(env!("OUT_DIR"), "/static-core-queries.rs"));

#[cfg(not(any(feature = "static-cmvs", feature = "static-adv32")))]
pub const STATIC_CORES: &[(&str, shinki_emu_abi::Query)] = &[];
