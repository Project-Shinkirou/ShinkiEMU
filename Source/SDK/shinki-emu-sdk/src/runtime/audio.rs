use std::sync::OnceLock;

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

/// PCM output for native family runtime audio workers.
pub trait AudioSink: Send + Sync {
    fn configure(&self, format: PcmFormatSpec) -> SessionResult<()>;
    /// Return the reusable source buffer, or None when cancellation stops playback.
    fn write(&self, samples: Vec<f32>) -> SessionResult<Option<Vec<f32>>>;
    fn cancel(&self) -> SessionResult<()>;
}

pub type AudioSinkBox = Box<dyn AudioSink>;

/// Opens one host-owned PCM output per family session.
pub type HostAudioOutput = fn() -> SessionResult<AudioSinkBox>;

static HOST_AUDIO_OUTPUT: OnceLock<HostAudioOutput> = OnceLock::new();

/// Installs the process-wide PCM output used by families statically embedded
/// into a host whose platform has no SDK device-audio backend. Families open
/// their own device when no host output is installed.
pub fn install_host_audio_output(open: HostAudioOutput) -> SessionResult<()> {
    HOST_AUDIO_OUTPUT.set(open).map_err(|_| {
        SessionError::invalid(
            "SHINKI_EMU_HOST_AUDIO_INSTALLED",
            "a host audio output is already installed",
        )
    })
}

/// Opens the installed host PCM output, or None when the family owns its device.
pub fn open_host_audio_output() -> Option<SessionResult<AudioSinkBox>> {
    HOST_AUDIO_OUTPUT.get().map(|open| open())
}
