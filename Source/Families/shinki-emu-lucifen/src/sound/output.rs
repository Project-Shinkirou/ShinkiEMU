use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(target_env = "ohos")]
use shinki_emu_sdk::{
    NativeAudioProgress,
    runtime::{AudioSink, OhosAudioOutput, PcmFormat, PcmFormatSpec},
};
#[cfg(target_env = "ohos")]
use std::{
    sync::{OnceLock, Weak},
    time::Duration,
};

#[derive(Clone, Default)]
pub(crate) struct AudioControl {
    pub(crate) stop: Arc<AtomicBool>,
    #[cfg(not(target_env = "ohos"))]
    pub(crate) null_device: bool,
    #[cfg(target_env = "ohos")]
    device: Arc<OnceLock<Weak<OhosAudioOutput>>>,
}

impl AudioControl {
    pub(crate) fn cancel(&self) {
        self.stop.store(true, Ordering::Release);
        #[cfg(target_env = "ohos")]
        if let Some(device) = self.device.get().and_then(Weak::upgrade) {
            let _ = device.cancel();
        }
    }
}

pub(crate) struct Output {
    #[cfg(not(target_env = "ohos"))]
    device: shinki_emu_sdk::NativeAudioOutput,
    #[cfg(target_env = "ohos")]
    device: Arc<OhosAudioOutput>,
}

impl Output {
    pub(crate) fn open(control: &AudioControl) -> io::Result<Self> {
        #[cfg(not(target_env = "ohos"))]
        let device = if control.null_device {
            shinki_emu_sdk::NativeAudioOutput::open_null()
        } else {
            shinki_emu_sdk::NativeAudioOutput::open(control.stop.clone())
        }
        .map_err(io::Error::other)?;
        #[cfg(target_env = "ohos")]
        let device = {
            let device = Arc::new(OhosAudioOutput::tracked(Duration::from_millis(500)));
            device
                .configure(PcmFormatSpec {
                    sample_rate: super::OUTPUT_RATE,
                    channels: 2,
                    format: PcmFormat::F32,
                })
                .map_err(io::Error::other)?;
            control
                .device
                .set(Arc::downgrade(&device))
                .map_err(|_| io::Error::other("Lucifen audio output is already open"))?;
            // Cancellation may have arrived before the configured renderer was published.
            if control.stop.load(Ordering::Acquire) {
                device.cancel().map_err(io::Error::other)?;
            }
            device
        };
        Ok(Self { device })
    }

    pub(super) fn write(
        &mut self,
        samples: Vec<f32>,
        stop: &AtomicBool,
    ) -> io::Result<Option<Vec<f32>>> {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        #[cfg(not(target_env = "ohos"))]
        let result = self.device.write(samples, stop);
        #[cfg(target_env = "ohos")]
        let result = self.device.write(samples);
        result.map_err(io::Error::other)
    }

    #[cfg(not(target_env = "ohos"))]
    pub(super) fn flush(&mut self, stop: &AtomicBool) -> io::Result<()> {
        self.device.flush(stop).map(|_| ()).map_err(io::Error::other)
    }

    #[cfg(target_env = "ohos")]
    pub(super) fn progress(&self) -> io::Result<NativeAudioProgress> {
        let progress = self.device.progress();
        self.device.check().map_err(io::Error::other)?;
        Ok(progress)
    }

    #[cfg(target_env = "ohos")]
    pub(crate) fn drain(&self) -> io::Result<()> {
        self.device.drain().map(|_| ()).map_err(io::Error::other)
    }
}
