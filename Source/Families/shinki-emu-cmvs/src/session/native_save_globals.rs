//! Slot 1760 belongs to the effect timeline, not the settings object. Audio
//! owners 772/784/792 are captured and prepared by native_save_audio.

use shinki_emu_sdk::runtime::SessionResult;

impl super::CmvsSession {
    /// Native 4262A0 renderer tick, or 47C040 skip-and-clear transaction.
    pub(super) fn advance_native_timeline(
        &mut self,
        skip: bool,
        reset_after: bool,
    ) -> SessionResult<()> {
        while let Some(request) =
            crate::vm::native_timeline::advance(&mut self.vm, skip).map_err(crate::family::core)?
        {
            use crate::vm::native_timeline::NativeTimelineTarget;
            match request.target {
                NativeTimelineTarget::Parent(parent) => {
                    crate::vm::texture_reload::begin(&mut self.vm, parent, request.duration)
                        .map_err(crate::family::core)?;
                    let previous_result = self.vm.interpreter_words.get(&81220).copied();
                    let loaded = self.scene.load_image_reload(
                        &mut self.vm,
                        parent,
                        request.resource,
                        crate::CmvsResourceSearchBank::Texture,
                        81220,
                        &self.archive,
                    );
                    match previous_result {
                        Some(word) => {
                            self.vm.write_interpreter_word(81220, word);
                        }
                        None => {
                            self.vm.interpreter_words.remove(&81220);
                        }
                    }
                    if loaded.map_err(crate::family::core)? {
                        super::record_completed_load(&mut self.vm);
                        self.last_loaded_parent = Some(parent);
                    }
                }
                NativeTimelineTarget::ChannelElement(child) => {
                    if self
                        .scene
                        .load_channel_image_reload(
                            &mut self.vm,
                            child,
                            request.resource,
                            request.duration,
                            &self.archive,
                        )
                        .map_err(crate::family::core)?
                    {
                        super::record_completed_load(&mut self.vm);
                    }
                }
            }
        }
        if reset_after {
            crate::vm::native_timeline::reset(&mut self.vm);
        }
        Ok(())
    }
}
