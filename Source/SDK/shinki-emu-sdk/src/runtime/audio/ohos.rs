use super::{AudioSink, PcmFormat, PcmFormatSpec, SessionError, SessionResult};
use std::sync::OnceLock;

#[repr(C)]
struct AudioStream {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    fn shinki_ohos_audio_open(
        stream: *mut *mut AudioStream,
        sample_rate: u32,
        channels: u32,
    ) -> i32;
    fn shinki_ohos_audio_write(stream: *mut AudioStream, samples: *const f32, count: usize) -> i32;
    fn shinki_ohos_audio_cancel(stream: *mut AudioStream);
    fn shinki_ohos_audio_close(stream: *mut AudioStream);
}

/// One OHAudio renderer per family session, paced by its bounded queue.
#[derive(Default)]
pub struct OhosAudioOutput {
    stream: OnceLock<OhosAudioStream>,
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

impl AudioSink for OhosAudioOutput {
    fn configure(&self, format: PcmFormatSpec) -> SessionResult<()> {
        format.validate()?;
        let PcmFormat::F32 = format.format;
        let mut stream = std::ptr::null_mut();
        let result = unsafe {
            shinki_ohos_audio_open(&mut stream, format.sample_rate, u32::from(format.channels))
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
        let stream = self.stream.get().ok_or_else(|| {
            SessionError::invalid(
                "SHINKI_EMU_OHOS_AUDIO_UNCONFIGURED",
                "audio output was not configured",
            )
        })?;
        match unsafe { shinki_ohos_audio_write(stream.0, samples.as_ptr(), samples.len()) } {
            0 => Ok(Some(samples)),
            1 => Ok(None),
            result => Err(SessionError::new(
                "SHINKI_EMU_OHOS_AUDIO_WRITE",
                format!("OHAudio queue rejected samples: {result}"),
            )),
        }
    }

    fn cancel(&self) -> SessionResult<()> {
        if let Some(stream) = self.stream.get() {
            unsafe { shinki_ohos_audio_cancel(stream.0) };
        }
        Ok(())
    }
}
