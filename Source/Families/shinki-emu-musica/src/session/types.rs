//! Musica-private owner startup and in-process PCM output values.
use shinki_emu_sdk::runtime::*;
use shinki_emu_sdk::runtime::{SessionError, SessionResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmFormat {
    F32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmFormatSpec {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: PcmFormat,
}

impl PcmFormatSpec {
    pub fn validate(&self) -> SessionResult<()> {
        if !(8_000..=192_000).contains(&self.sample_rate) || self.channels == 0 {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_PCM_FORMAT",
                "PCM sample rate or channel count is outside the supported range",
            ));
        }
        Ok(())
    }
}


/// In-process PCM output used by deterministic runtime tests. Production owners open their own device.
pub trait AudioSink: Send + Sync {
    fn configure(&self, format: PcmFormatSpec) -> SessionResult<()>;
    /// Return the reusable source buffer, or None when cancellation stops playback.
    fn write(&self, samples: Vec<f32>) -> SessionResult<Option<Vec<f32>>>;
    fn cancel(&self) -> SessionResult<()>;
}

pub type AudioSinkBox = Box<dyn AudioSink>;

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
