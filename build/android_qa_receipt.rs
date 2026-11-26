//! Private to the single-ABI Android QA packager and Manager local ZIP import.
//! The APK signature covers the receipt; ZIPs are distributed separately.

use serde::{Deserialize, Serialize};
use shinki_emu_metadata::CoreIdentity;
use std::collections::BTreeMap;

pub const SCHEMA: &str = "shinki.emu.android_qa_core_set.v1";
pub const ASSET: &str = "android-qa-core-set.json";
pub const MAX_BYTES: usize = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub abi: String,
    pub product_version: String,
    pub source_commit: String,
    pub cores: Vec<Core>,
    pub omitted: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Core {
    pub plugin_id: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<CoreIdentity>,
    pub size: u64,
    pub sha256: String,
}

impl Receipt {
    #[allow(dead_code)] // Only the Manager reads the APK asset.
    pub fn read_debug_asset(
        debuggable: bool,
        read: impl FnOnce() -> Option<Vec<u8>>,
        abi: &str,
        version: &str,
        commit: &str,
    ) -> Option<Self> {
        if !debuggable {
            return None;
        }
        let bytes = read()?;
        if bytes.len() > MAX_BYTES {
            return None;
        }
        let receipt: Self = serde_json::from_slice(&bytes).ok()?;
        receipt.matches_product(abi, version, commit).then_some(receipt)
    }

    pub fn matches_product(&self, abi: &str, version: &str, commit: &str) -> bool {
        self.schema == SCHEMA
            && machine(abi).is_some()
            && self.abi == abi
            && self.product_version == version
            && self.source_commit == commit
            && is_lower_hex(commit, 40)
            && !self.cores.is_empty()
            && self.cores.windows(2).all(|pair| pair[0].plugin_id < pair[1].plugin_id)
            && self.cores.iter().all(|core| {
                (1..=shinki_emu_metadata::MAX_CORE_PACKAGE_BYTES).contains(&core.size)
                    && is_lower_hex(&core.sha256, 64)
                    && !self.omitted.contains_key(&core.plugin_id)
                    && (core.identity.is_some() || core.version == version)
                    && shinki_emu_metadata::validate_core_version(&core.version, core.identity.as_ref()).is_ok()
            })
    }
}

fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn machine(abi: &str) -> Option<u16> {
    match abi {
        "arm64-v8a" => Some(183),
        "x86_64" => Some(62),
        _ => None,
    }
}

pub fn android_shared_object_header(header: &[u8; 64], machine: u16) -> bool {
    header[..7] == *b"\x7fELF\x02\x01\x01"
        && u16::from_le_bytes([header[16], header[17]]) == 3 // ET_DYN
        && u16::from_le_bytes([header[18], header[19]]) == machine
        && u32::from_le_bytes(header[20..24].try_into().unwrap()) == 1
}
