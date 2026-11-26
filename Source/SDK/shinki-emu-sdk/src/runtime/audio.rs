#[cfg(target_env = "ohos")]
mod ohos;
#[cfg(target_env = "ohos")]
pub use ohos::OhosAudioOutput;

use super::{SessionError, SessionResult};

/// Timeline in source PCM frames. Underrun silence does not advance it.
#[cfg(any(feature = "device-audio", target_env = "ohos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct NativeAudioProgress {
    /// Source frames queued by the writer (including any explicit flush padding).
    pub accepted_frames: u64,
    /// Source frames presented by the device; monotonic and `<= accepted_frames`.
    pub played_frames: u64,
}

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

/// PCM output for native family runtime audio workers.
pub trait AudioSink: Send + Sync {
    fn configure(&self, format: PcmFormatSpec) -> SessionResult<()>;
    /// Return the reusable source buffer, or None when cancellation stops playback.
    fn write(&self, samples: Vec<f32>) -> SessionResult<Option<Vec<f32>>>;
    fn cancel(&self) -> SessionResult<()>;
}

pub type AudioSinkBox = Box<dyn AudioSink>;
