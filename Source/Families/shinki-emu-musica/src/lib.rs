mod configuration;
pub use configuration::*;
mod audio;
mod movie;
mod native_directory;
mod provider;
mod scene;
mod session;
mod storage;
mod voice_preferences;

mod archive;
mod embed;
mod image_container;
mod localization;
pub use embed::embed_sc_localization;
mod message;
mod name_resources;
pub use name_resources::MusicaNameResources;
mod subtitles;
pub use localization::MusicaTranslationTable;
mod paz;
mod profile;
mod resources;
pub use resources::MusicaResourcePaths;
mod runtime;
pub use message::*;
mod script_loader;
pub use script_loader::{ScriptValidation, read_expanded_script, validate_musica_script};
mod text_renderer;

pub use archive::*;
pub use image_container::*;
pub use paz::*;
pub use profile::*;
pub use runtime::title::validate_musica_title_frontend;
pub use runtime::*;
pub use shinki_emu_musica_format::{
    ArchiveEntryDescriptor, MAX_ENTRY_BYTES, MAX_INDEX_BYTES, MUSICA_SCRIPT_IR_SCHEMA,
    PazRoleScheme, ScCensus, ScCommand, ScControlFlow, ScControlFlowKind, ScLine,
    ScLineKind, ScOpcodeCatalog, ScOpcodeSpec, ScOperand, ScParseError, ScScript,
    ScriptEncoding, SourceSpan, disassemble_sc, encode_sc, parse_sc,
    parse_sc_with_encoding, musica_display_cstr_hash, musica_display_hash,
};
pub use shinki_emu_sdk::CoreError;
pub use text_renderer::MusicaTextRenderer;

pub const MUSICA_READER_ID: &str = "shinki.emu.musica.paz.v1";
pub const MUSICA_DECRYPT_PROVIDER_ID: &str = "shinki.emu.musica.paz.decrypt.v1";
pub const MUSICA_DECRYPT_DESCRIPTOR_SCHEMA: &str = "shinki.emu.musica.paz.decrypt_descriptor.v1";
#[cfg(test)]
#[path = "script/tests.rs"]
mod script_tests;

mod native;
pub use native::musica_query;

#[cfg(test)]
pub(crate) use provider::{MusicaProvider, musica_descriptor};

#[cfg(test)]
mod test_fixture;

pub mod eden_save;
pub mod minori_data;
