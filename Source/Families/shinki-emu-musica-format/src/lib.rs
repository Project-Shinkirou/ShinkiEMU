//! Musica resource decoding and SC parsing shared by recognition and playback.
//! No VM, native ABI, filesystem loader, device or script execution dependencies.
#![forbid(unsafe_code)]

pub mod paz;
mod script;
pub use paz::{ArchiveEntryDescriptor, PazRoleScheme, MAX_ENTRY_BYTES, MAX_INDEX_BYTES};
pub use script::*;
mod schemes;
pub use schemes::{into_schemes, decode_scheme};

pub const MUSICA_SCRIPT_IR_SCHEMA: &str = "shinki.emu.musica.script_ir.v3";
