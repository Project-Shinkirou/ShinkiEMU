mod elg;
mod lag;
mod lgm;
mod lpk;
mod save;
mod sob;
mod user;

pub use lpk::{Archive, CHUNK, Entry, Replacements, SCRIPT_ARCHIVE, has_script_index, member_key};
pub use save::{SaveSlot, SaveSlotPreview, THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH};
pub use sob::{Bootstrap, ModuleKind, Relocation, Sob};

pub use elg::{ElgImage, decode_elg};
pub use lag::{AnimationFrame, Lag};
pub use lgm::{LayoutControl, Lgm};

pub use user::{UserVariables, Variable};

pub const FORMAT_ID: &str = "lucifen.lpk1-sob";

/// Decode a Lucifen LZSS block into exactly the native declared length.
pub fn decode_lzss(input: &[u8], size: usize, limit: usize) -> std::io::Result<Vec<u8>> {
    if size > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Lucifen LZSS output exceeds its limit",
        ));
    }
    let mut output = Vec::with_capacity(size);
    lpk::lzss(input, size, &mut output)?;
    Ok(output)
}
