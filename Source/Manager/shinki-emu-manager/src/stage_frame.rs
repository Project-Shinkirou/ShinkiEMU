use super::*;
use shinki_emu_manager_core::model::{FamilyResult, FrameIdentity, FrameInfo, FrameLease};

/// Latest complete frame; CPU bytes and GPU targets both remain leased.
#[derive(Clone, Default)]
pub(crate) struct FrameMailbox {
    inner: Arc<FrameMailboxInner>,
}

#[derive(Default)]
pub(super) struct FrameMailboxInner {
    state: Mutex<FrameMailboxState>,
    pub(super) gpu: Mutex<super::gpu::GpuStage>,
}

#[derive(Clone)]
pub(super) struct GpuFrame {
    pub(super) identity: FrameIdentity,
    pub(super) info: FrameInfo,
    pub(super) texture: wgpu::Texture,
    pub(super) _lease: Arc<()>,
}

#[derive(Clone)]
pub(super) enum StageFrame {
    Cpu(FrameLease),
    Gpu(GpuFrame),
}

impl StageFrame {
    pub(super) fn identity(&self) -> FrameIdentity {
        match self {
            Self::Cpu(frame) => frame.identity,
            Self::Gpu(frame) => frame.identity,
        }
    }

    pub(super) fn info(&self) -> FrameInfo {
        match self {
            Self::Cpu(frame) => frame.info,
            Self::Gpu(frame) => frame.info,
        }
    }
}

impl FrameMailboxInner {
    pub(super) fn require_epoch(&self, epoch: u64) -> Result<(), shinki_emu_abi::Status> {
        let state = self.state.lock().map_err(|_| shinki_emu_abi::PANICKED)?;
        if state.session_epoch != Some(epoch) {
            return Err(shinki_emu_abi::STALE_CONTEXT);
        }
        Ok(())
    }

    pub(super) fn publish_gpu(
        &self,
        session_epoch: u64,
        info: FrameInfo,
        texture: wgpu::Texture,
        lease: Arc<()>,
    ) -> Result<(), shinki_emu_abi::Status> {
        let mut state = self.state.lock().map_err(|_| shinki_emu_abi::PANICKED)?;
        let identity = state.next_scene_identity(session_epoch)?;
        state.frame = Some(StageFrame::Gpu(GpuFrame { identity, info, texture, _lease: lease }));
        state.scene_output = true;
        Ok(())
    }

    pub(super) fn publish_native_raster(
        &self,
        session_epoch: u64,
        info: FrameInfo,
        pixels: roukaku_media_core::OwnedPixelBuffer,
    ) -> Result<(), shinki_emu_abi::Status> {
        let mut state = self.state.lock().map_err(|_| shinki_emu_abi::PANICKED)?;
        let identity = state.next_scene_identity(session_epoch)?;
        let frame = FrameLease::from_owner(pixels, |pixels| pixels.as_slice(), info, identity)
            .map_err(|_| shinki_emu_abi::INVALID_ARGUMENT)?;
        state.frame = Some(StageFrame::Cpu(frame));
        state.scene_output = true;
        Ok(())
    }
}

#[derive(Default)]
struct FrameMailboxState {
    session_epoch: Option<u64>,
    gpu_committed: bool,
    scene_output: bool,
    frame: Option<StageFrame>,
    displayed: Option<(FrameIdentity, FrameInfo)>,
}

impl FrameMailboxState {
    fn visible_frame(&self) -> Option<&StageFrame> {
        self.frame.as_ref().filter(|_| self.gpu_committed || !self.scene_output)
    }

    fn next_scene_identity(&self, session_epoch: u64) -> Result<FrameIdentity, shinki_emu_abi::Status> {
        use shinki_emu_abi as abi;
        if self.session_epoch != Some(session_epoch) {
            return Err(abi::STALE_CONTEXT);
        }
        let generation = match &self.frame {
            None => 1,
            Some(previous) if self.scene_output => previous.identity().generation
                .checked_add(1).ok_or(abi::INVALID_STATE)?,
            Some(_) => return Err(abi::INVALID_STATE),
        };
        Ok(FrameIdentity { session_epoch, output_epoch: 1, generation })
    }
}

impl FrameMailbox {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn activate(&self, session_epoch: u64) -> FamilyResult<()> {
        FrameIdentity {
            session_epoch,
            output_epoch: 0,
            generation: 0,
        }
        .validate(false)?;
        let mut state = self.inner.state.lock().map_err(|_| lock_error())?;
        state.session_epoch = Some(session_epoch);
        state.gpu_committed = false;
        state.scene_output = false;
        state.frame = None;
        state.displayed = None;
        Ok(())
    }

    pub(crate) fn publish(&self, frame: FrameLease) -> FamilyResult<()> {
        frame.validate()?;
        let mut state = self.inner.state.lock().map_err(|_| lock_error())?;
        if state.session_epoch != Some(frame.identity.session_epoch) {
            return Err(shinki_emu_manager_core::model::FamilyError::invalid(
                "SHINKI_EMU_HOST_FRAME_EPOCH",
                "frame belongs to an inactive session",
            ));
        }
        if state.scene_output {
            return Err(shinki_emu_manager_core::model::FamilyError::invalid(
                "SHINKI_EMU_HOST_FRAME_TRANSPORT",
                "frame transport changed during the session",
            ));
        }
        if let Some(StageFrame::Cpu(previous)) = &state.frame {
            if frame.identity.output_epoch < previous.identity.output_epoch
                || (frame.identity.output_epoch == previous.identity.output_epoch
                    && frame.identity.generation < previous.identity.generation)
            {
                return Err(shinki_emu_manager_core::model::FamilyError::invalid(
                    "SHINKI_EMU_HOST_FRAME_GENERATION",
                    "completed frame generation moved backwards",
                ));
            }
            if frame.identity == previous.identity {
                if frame.info != previous.info || !frame.same_storage(previous) {
                    return Err(shinki_emu_manager_core::model::FamilyError::invalid(
                        "SHINKI_EMU_HOST_FRAME_GENERATION",
                        "same frame generation refers to different output",
                    ));
                }
                return Ok(());
            }
        }
        state.frame = Some(StageFrame::Cpu(frame));
        Ok(())
    }

    pub(crate) fn identity(&self) -> Result<Option<FrameIdentity>, String> {
        self.inner.state
            .lock()
            .map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.visible_frame().map(StageFrame::identity))
    }

    pub(crate) fn scene_identity(&self) -> Result<Option<FrameIdentity>, String> {
        let state = self.inner.state.lock().map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK")?;
        Ok(state.visible_frame().filter(|_| state.scene_output).map(StageFrame::identity))
    }

    pub(crate) fn commit_gpu_open(&self, epoch: u64) -> Result<(), String> {
        let mut state = self.inner.state.lock().map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK")?;
        if state.session_epoch != Some(epoch) {
            return Err("SHINKI_EMU_HOST_FRAME_EPOCH".into());
        }
        state.gpu_committed = true;
        Ok(())
    }

    pub(crate) fn gpu_service(&self) -> Option<shinki_emu_abi::GpuSceneServiceV2> {
        self.inner.gpu.lock().ok()?.available().then(|| super::gpu::service(&self.inner))
    }

    pub(super) fn setup_gpu(&self, context: &WgpuFrameContext<'_>) -> Result<(), String> {
        let mut gpu = self.inner.gpu.lock().map_err(|_| "SHINKI_EMU_HOST_GPU_SCENE_LOCK")?;
        gpu.setup(context.device, context.queue);
        Ok(())
    }

    pub(super) fn teardown_gpu(&self) {
        self.inner.gpu.lock().unwrap_or_else(std::sync::PoisonError::into_inner).teardown();
        let mut state = self.inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(&state.frame, Some(StageFrame::Gpu(_))) {
            state.frame = None;
            state.displayed = None;
        }
    }
    pub(crate) fn presented(
        &self,
        identity: FrameIdentity,
        info: shinki_emu_manager_core::model::FrameInfo,
    ) -> Result<(), String> {
        let first = {
            let mut state = self.inner.state.lock().map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK")?;
            if state.session_epoch != Some(identity.session_epoch) {
                return Ok(());
            }
            state.displayed.replace((identity, info)).is_none()
        };
        if first {
            // This is the renderer's acceptance boundary, not a Slint or
            // display presentation receipt.
            tracing::info!(
                event = "shinki.emu.host.first_stage_frame_accepted",
                session_epoch = identity.session_epoch,
                output_epoch = identity.output_epoch,
                generation = identity.generation,
                width = info.width,
                height = info.height,
                logical_width = info.logical_width,
                logical_height = info.logical_height,
            );
        }
        Ok(())
    }
    pub(crate) fn displayed_identity(&self) -> Result<Option<FrameIdentity>, String> {
        self.inner.state
            .lock()
            .map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.displayed.map(|v| v.0))
    }
    pub(crate) fn displayed(
        &self,
    ) -> Result<Option<(FrameIdentity, shinki_emu_manager_core::model::FrameInfo)>, String> {
        self.inner.state
            .lock()
            .map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.displayed)
    }

    /// Late shutdown from an old owner cannot clear the new session baseline.
    pub(crate) fn clear(&self, session_epoch: u64) -> Result<(), String> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK".to_owned())?;
        if state.session_epoch == Some(session_epoch) {
            state.frame = None;
            state.session_epoch = None;
            state.gpu_committed = false;
            state.scene_output = false;
            state.displayed = None;
        }
        Ok(())
    }

    pub(super) fn stage_snapshot(&self) -> Result<Option<StageFrame>, String> {
        self.inner.state
            .lock()
            .map_err(|_| "SHINKI_EMU_HOST_FRAME_LOCK".to_owned())
            .map(|state| state.visible_frame().cloned())
    }

    #[cfg(any(test, target_env = "ohos"))]
    pub(crate) fn snapshot(&self) -> Result<Option<FrameLease>, String> {
        Ok(match self.stage_snapshot()? {
            Some(StageFrame::Cpu(frame)) => Some(frame),
            _ => None,
        })
    }
}

fn lock_error() -> shinki_emu_manager_core::model::FamilyError {
    shinki_emu_manager_core::model::FamilyError::invalid(
        "SHINKI_EMU_HOST_FRAME_LOCK",
        "frame mailbox lock is poisoned",
    )
}
