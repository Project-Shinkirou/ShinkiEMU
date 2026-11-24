//! Native interpreter for CIRCUS ADV32.

mod animation;
mod archive;
mod audio;
mod authorization;
mod binary;
mod comments;
mod crx;
mod effects;
mod game;
mod graphics;
mod mes;
mod message;
#[cfg(feature = "video-ffmpeg")]
mod movie;
mod native;
mod native_data;
mod render;
mod save;
mod text;
mod vm;
mod xpcm;

pub use native::adv32_query;
#[cfg(feature = "dynamic-plugin-export")]
pub use native::shinki_emu_query;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("unsupported {0}")]
    Unsupported(&'static str),
    #[error("unsupported ADV32 MES version 0x{0:04x}")]
    UnsupportedMesVersion(u16),
    #[error("missing ADV32 asset {0}")]
    MissingAsset(String),
    #[cfg(windows)]
    #[error(
        "ADV32 requires its original optical disc. Windows registry diskless authorization is unsupported."
    )]
    DiscRequired,
    #[error("ADV32 surface {0} has not been loaded")]
    MissingSurface(u8),
    #[error("ADV32 audio: {0}")]
    Audio(String),
    #[error("ADV32 image: {0}")]
    Image(String),
    #[error("ADV32 render: {0}")]
    Render(String),
    #[error("ADV32 movie: {0}")]
    #[cfg(feature = "video-ffmpeg")]
    Movie(String),
    #[error("unsupported ADV32 command 0x{opcode:02x} in {script} at 0x{offset:x}")]
    UnsupportedCommand {
        opcode: u8,
        script: String,
        offset: usize,
    },
    #[error("ADV32 script {script} at 0x{offset:x}: {source}")]
    ScriptExecution {
        script: String,
        offset: usize,
        source: Box<Error>,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, Error>;
