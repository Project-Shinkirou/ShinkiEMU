use super::*;
impl MusicaSession {
    pub(crate) fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[RuntimeEvent],
    ) -> SessionResult<AdvanceResponse> {
        if self.poisoned {
            return Err(error(
                "SHINKI_EMU_MUSICA_POISONED",
                "failed session must be closed",
            ));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.advance_inner(elapsed_ns, events)
        }))
        .unwrap_or_else(|_| {
            Err(error(
                "SHINKI_EMU_MUSICA_SESSION_PANIC",
                "session panicked and must be closed",
            ))
        });
        if result.is_err() && !self.hook_failed {
            self.poisoned = true;
        }
        if result.as_ref().is_ok_and(|response| response.reset_clock) {
            self.force_frame = true;
        }
        result
    }
    #[cfg(test)]
    pub(crate) fn acquire_frame(
        &mut self,
        after: Option<FrameIdentity>,
    ) -> SessionResult<FramePoll> {
        if self.poisoned {
            return Err(error(
                "SHINKI_EMU_MUSICA_POISONED",
                "failed session must be closed",
            ));
        }
        let pixels = &self.scene.pixels;
        if self.force_frame
            || self.published_frame.current().is_none_or(|frame| {
                frame.info != self.info || frame.pixels().as_ptr() != pixels.as_ptr()
            })
        {
            self.published_frame.publish(
                Arc::clone(pixels),
                |pixels| pixels.as_ref(),
                self.info,
            )?;
            self.force_frame = false;
        }
        Ok(self.published_frame.poll(after))
    }
    pub(crate) fn close(mut self: Box<Self>) -> SessionResult<()> {
        let text = self.cancel_text();
        let movie = self
            .movie
            .take()
            .map(|movie| movie.close(&self.audio))
            .transpose();
        let audio = self.audio.shutdown();
        tracing::info!(event = "shinki.emu.musica.session.close");
        text?;
        movie?;
        audio
    }
}
impl Drop for MusicaSession {
    fn drop(&mut self) {
        let _ = self.cancel_text();
        if let Some(movie) = self.movie.take() {
            let _ = movie.close(&self.audio);
        }
        let _ = self.audio.shutdown();
    }
}

pub(super) fn vm_error(cause: crate::MusicaRuntimeError) -> SessionError {
    if let crate::MusicaRuntimeError::UnsupportedOpcode { ordinal, .. } = &cause {
        return error(
            cause.diagnostic_code(),
            &format!("script command at ordinal {ordinal} is not implemented"),
        );
    }
    error(
        cause.diagnostic_code(),
        "script execution failed at an unsupported or invalid operation",
    )
}
