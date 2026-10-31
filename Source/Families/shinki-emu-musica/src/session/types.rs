//! Musica-private owner startup values.
use shinki_emu_sdk::runtime::*;
pub struct OpenRequest {
    pub game_path: String,
    pub session_epoch: u64,
    pub configuration: Vec<ConfigEntry>,
    pub initial_window: WindowState,
    pub audio_output: Option<Box<dyn AudioSink>>,
}

impl OpenRequest {
    pub fn validate(&self) -> SessionResult<()> {
        shinki_emu_sdk::runtime::validate_game_path(&self.game_path)?;
        FrameIdentity {
            session_epoch: self.session_epoch,
            generation: 0,
        }
        .validate(false)?;
        self.initial_window.validate()
    }
}

#[derive(Debug, Clone)]
pub struct OpenResponse {
    pub frame: FrameInfo,
    pub audio_format: Option<PcmFormatSpec>,
}

impl OpenResponse {
    pub fn validate(&self) -> SessionResult<()> {
        self.frame.validate()?;
        if let Some(audio) = self.audio_format {
            audio.validate()?;
        }
        Ok(())
    }
}
