use super::{SessionError, SessionResult};

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

/// Host-provided PCM output for native family runtime audio workers.
pub trait AudioSink: Send + Sync {
    fn configure(&self, format: PcmFormatSpec) -> SessionResult<()>;
    /// Return the reusable source buffer, or None when cancellation stops playback.
    fn write(&self, samples: Vec<f32>) -> SessionResult<Option<Vec<f32>>>;
    fn cancel(&self) -> SessionResult<()>;
}

pub type AudioSinkBox = Box<dyn AudioSink>;
