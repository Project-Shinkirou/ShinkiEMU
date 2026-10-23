use crate::{audio::Audio, scene::error, MusicaMountedVfs, MusicaMovieState};
use roukaku_media_core::TextureFrame;
use shinki_emu_sdk::runtime::SessionResult;

#[cfg(feature = "ffmpeg-vcpkg")]
mod playback;
#[cfg(feature = "ffmpeg-vcpkg")]
pub(crate) use playback::Movie;

#[cfg(not(feature = "ffmpeg-vcpkg"))]
pub(crate) struct Movie;
#[cfg(not(feature = "ffmpeg-vcpkg"))]
impl Movie {
    pub fn open(_: &std::sync::Arc<MusicaMountedVfs>, _: &MusicaMovieState) -> SessionResult<Self> {
        Err(error(
            "SHINKI_EMU_MUSICA_MOVIE_UNAVAILABLE",
            "movie playback requires ffmpeg-vcpkg",
        ))
    }
    pub fn advance(&mut self, _: &Audio, _: u64, _: bool) -> SessionResult<Option<TextureFrame>> {
        unreachable!()
    }
    pub fn position_us(&self) -> u64 {
        unreachable!()
    }
    pub fn ended(&self) -> bool {
        unreachable!()
    }
    pub fn close(self, _: &Audio) -> SessionResult<()> {
        unreachable!()
    }
}
