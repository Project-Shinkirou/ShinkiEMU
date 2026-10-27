//! CMVS formats, recovered VM, and core-owned native C ABI sessions.
mod archive;
mod audio;
mod profile;
pub use profile::{
    CMVS_PROFILE_FILE, CMVS_PROFILE_SCHEMA, CmvsArchiveFile, CmvsProfile, mount_cmvs,
};
mod cmvs_md5;
pub use archive::CmvsArchive;
pub mod cmv;
mod cpz;
mod jbp;
mod mgv;
mod pb;
mod ps2a;
mod scene;
mod scheme;
mod script;
pub use scene::{CmvsScene, CmvsTextureSlot};
pub(crate) mod system_save;
pub use cpz::*;
pub use mgv::*;
pub use pb::*;
pub use ps2a::*;
pub use scheme::*;
pub use system_save::*;

pub const CMVS_FAMILY_ID: &str = "cmvs";
pub const CMVS_READER_ID: &str = "shinki.emu.cmvs.reader.v1";
pub const CMVS_DECRYPT_PROVIDER_ID: &str = "shinki.emu.cmvs.cpz.decrypt.v1";

mod command;
mod vm;
pub use command::*;
pub use vm::*;

pub const CMVS_DECRYPT_DESCRIPTOR_SCHEMA: &str = "shinki.emu.cmvs.cpz5_descriptor.v1";

mod family;
mod native;
mod session;
pub use native::cmvs_query;
