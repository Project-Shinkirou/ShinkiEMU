use super::{
    AudioSink, NativeAudioProgress, PcmFormat, PcmFormatSpec, SessionError, SessionResult,
};
use std::{sync::OnceLock, time::Duration};

const CANCELLED: i32 = -2;

#[repr(C)]
struct AudioStream {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    fn shinki_ohos_audio_open(
        stream: *mut *mut AudioStream,
        sample_rate: u32,
        channels: u32,
        stall_ms: u32,
    ) -> i32;
    fn shinki_ohos_audio_write(stream: *mut AudioStream, samples: *const f32, count: usize) -> i32;
    fn shinki_ohos_audio_progress(stream: *mut AudioStream, progress: *mut NativeAudioProgress);
    fn shinki_ohos_audio_check(stream: *mut AudioStream) -> i32;
    fn shinki_ohos_audio_drain(stream: *mut AudioStream) -> i32;
    fn shinki_ohos_audio_cancel(stream: *mut AudioStream);
    fn shinki_ohos_audio_close(stream: *mut AudioStream);
}

/// One OHAudio renderer per family session, paced by its bounded queue.
#[derive(Default)]
pub struct OhosAudioOutput {
    stream: OnceLock<OhosAudioStream>,
    stall_ms: u32,
}

struct OhosAudioStream(*mut AudioStream);

// The C stream serializes every access behind its own mutex.
unsafe impl Send for OhosAudioStream {}
unsafe impl Sync for OhosAudioStream {}

impl Drop for OhosAudioStream {
    fn drop(&mut self) {
        unsafe { shinki_ohos_audio_close(self.0) };
    }
}

impl OhosAudioOutput {
    /// Enable the presentation clock, drain, and a bound on stalled device waits.
    pub fn tracked(stall_timeout: Duration) -> Self {
        Self {
            stream: OnceLock::new(),
            stall_ms: stall_timeout.as_millis().clamp(1, u128::from(u32::MAX)) as u32,
        }
    }

    /// The last known timeline, including while the writer is blocked.
    /// Call `check` separately for device failure.
    pub fn progress(&self) -> NativeAudioProgress {
        let mut progress = NativeAudioProgress::default();
        if let Some(stream) = self.stream.get() {
            unsafe { shinki_ohos_audio_progress(stream.0, &mut progress) };
        }
        progress
    }

    pub fn check(&self) -> SessionResult<()> {
        let stream = self.configured()?;
        stream_result(unsafe { shinki_ohos_audio_check(stream.0) }).map(|_| ())
    }

    /// Wait for every frame already accepted by a tracked output to be presented.
    /// Returns `None` on cancellation, never on device failure.
    pub fn drain(&self) -> SessionResult<Option<()>> {
        let stream = self.configured()?;
        stream_result(unsafe { shinki_ohos_audio_drain(stream.0) })
    }

    fn configured(&self) -> SessionResult<&OhosAudioStream> {
        self.stream.get().ok_or_else(|| {
            SessionError::invalid(
                "SHINKI_EMU_OHOS_AUDIO_UNCONFIGURED",
                "audio output was not configured",
            )
        })
    }
}

fn stream_result(status: i32) -> SessionResult<Option<()>> {
    match status {
        0 => Ok(Some(())),
        CANCELLED => Ok(None),
        -3 => Err(SessionError::new(
            "SHINKI_EMU_OHOS_AUDIO_STALLED",
            "OHAudio renderer stopped making progress",
        )),
        code => Err(SessionError::new(
            "SHINKI_EMU_OHOS_AUDIO_STREAM",
            format!("OHAudio renderer or queue failed: {code}"),
        )),
    }
}

impl AudioSink for OhosAudioOutput {
    fn configure(&self, format: PcmFormatSpec) -> SessionResult<()> {
        format.validate()?;
        let PcmFormat::F32 = format.format;
        let mut stream = std::ptr::null_mut();
        let result = unsafe {
            shinki_ohos_audio_open(
                &mut stream,
                format.sample_rate,
                u32::from(format.channels),
                self.stall_ms,
            )
        };
        if result != 0 || stream.is_null() {
            return Err(SessionError::new(
                "SHINKI_EMU_OHOS_AUDIO_OPEN",
                format!("OHAudio renderer could not start: {result}"),
            ));
        }
        self.stream.set(OhosAudioStream(stream)).map_err(|_| {
            SessionError::invalid(
                "SHINKI_EMU_OHOS_AUDIO_CONFIGURED",
                "audio output is already configured",
            )
        })?;
        tracing::info!(
            event = "shinki.emu.ohos.audio_started",
            sample_rate = format.sample_rate,
            channels = format.channels
        );
        Ok(())
    }

    fn write(&self, samples: Vec<f32>) -> SessionResult<Option<Vec<f32>>> {
        let stream = self.configured()?;
        stream_result(unsafe { shinki_ohos_audio_write(stream.0, samples.as_ptr(), samples.len()) })
            .map(|written| written.map(|()| samples))
    }

    fn cancel(&self) -> SessionResult<()> {
        if let Some(stream) = self.stream.get() {
            unsafe { shinki_ohos_audio_cancel(stream.0) };
        }
        Ok(())
    }
}
