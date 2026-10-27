use super::*;

pub(crate) const PRESENTATION_HANDLE: u32 = 3260;
pub(crate) const CLEAR_SURFACE_URI: &str = "cmvs:/generated/presentation-clear";

/// `sub_458F10`: mask and clock belong to the transition, not the source image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TexturePresentation {
    pub parent: u8,
    pub mask: String,
    pub duration_ms: u32,
    pub elapsed_ms: u32,
    pub showing: bool,
}

impl TexturePresentation {
    pub(crate) fn remaining_ms(&self) -> u32 {
        self.duration_ms.saturating_sub(self.elapsed_ms)
    }

    /// GPU path `sub_459340/4594C0`: threshold traverses 0..2 or 2..0.
    pub(crate) fn alpha_lut(&self) -> [u8; 256] {
        let progress = if self.duration_ms == 0 {
            1.0
        } else {
            self.elapsed_ms as f32 / self.duration_ms as f32
        };
        let threshold = 2.0
            * if self.showing {
                progress
            } else {
                1.0 - progress
            };
        std::array::from_fn(|mask| {
            ((threshold - mask as f32 / 255.0).clamp(0.0, 1.0) * 255.0).round() as u8
        })
    }
}

pub(crate) fn begin(
    state: &mut CmvsPs2aVmState,
    mask: String,
    parent: u32,
    duration_ms: u32,
    showing: bool,
    clear_background: bool,
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    let parent = if parent == u32::MAX { 0 } else { parent };
    if parent >= SLOT_OBJECT_COUNT {
        *state
            .interpreter_words
            .entry(u32::from(INTERPRETER_ERROR_FLAG_OFFSET))
            .or_default() |= INTERPRETER_ERROR_FLAG_MASK;
        return Ok(None);
    }
    // Replacing an owner cancels its transition without destroying its image.
    state.texture_presentation = None;
    if clear_background {
        state.texture_children.remove(&0);
        state.texture_reloads.remove(&0);
        state.texture_parents.insert(
            0,
            CmvsTextureParentState {
                surface_initialized: true,
                ..Default::default()
            },
        );
        let identity = state.next_slot_object_id;
        state.next_slot_object_id += 1;
        state.slot_objects.insert(1924 << 8, identity);
        state.slot_object_seed_words.remove(&(1924 << 8));
    }
    state.texture_presentation = Some(TexturePresentation {
        parent: parent as u8,
        mask,
        duration_ms,
        elapsed_ms: 0,
        showing,
    });
    state.write_interpreter_word(PRESENTATION_HANDLE, 1);
    state.write_interpreter_word(81220, 1);
    Ok(Some(CmvsPs2aVmAction::PrepareTexturePresentation {
        clear_background,
    }))
}

/// Natural completion (`sub_4262A0`) and explicit completion (`sub_47AE00`).
pub(crate) fn finish(state: &mut CmvsPs2aVmState) -> Option<CmvsPs2aVmAction> {
    state.write_interpreter_word(PRESENTATION_HANDLE, 0);
    let presentation = state.texture_presentation.take()?;
    if presentation.showing {
        None
    } else {
        let parent = presentation.parent;
        state.slot_objects.remove(&(1924 << 8 | u32::from(parent)));
        state
            .slot_object_seed_words
            .remove(&(1924 << 8 | u32::from(parent)));
        state.texture_parents.remove(&parent);
        state.texture_children.remove(&parent);
        state.texture_reloads.remove(&parent);
        Some(CmvsPs2aVmAction::RemoveTextureParentSurface {
            parent_slot: parent,
        })
    }
}

pub(crate) fn advance(state: &mut CmvsPs2aVmState, elapsed_ms: u32) -> Option<CmvsPs2aVmAction> {
    let presentation = state.texture_presentation.as_mut()?;
    presentation.elapsed_ms = presentation
        .elapsed_ms
        .saturating_add(elapsed_ms)
        .min(presentation.duration_ms);
    if presentation.remaining_ms() == 0 {
        finish(state)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> CmvsPs2aVmState {
        let mut state = CmvsPs2aVmState::new(0);
        state
            .frame_string_pool_bytes
            .insert(0, b"mask.pb3\0".to_vec());
        state.script_frames.insert(
            0,
            CmvsScriptFrameIdentity {
                script_uri: "cmvs:/fixture/scene.ps3".into(),
                script_hash: roukaku_core::Hash256::from_sha256(b"scene"),
            },
        );
        for parent in [7, 28] {
            state
                .slot_objects
                .insert(1924 << 8 | u32::from(parent), u64::from(parent) + 1);
            state.texture_parents.insert(
                parent,
                CmvsTextureParentState {
                    resource: Some(format!("image{parent}.pb3")),
                    resource_frame: Some(0),
                    surface_initialized: true,
                    ..Default::default()
                },
            );
        }
        state.next_slot_object_id = 30;
        state
    }

    fn command(state: &mut CmvsPs2aVmState, id: u16) -> Option<CmvsPs2aVmAction> {
        state.dispatch_stopped = false;
        state.wait_frame = false;
        let pc = state.program_counter;
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: crate::CmvsPs2aSourceSpan {
                    offset: pc,
                    byte_length: 2,
                },
                command_id: id,
            },
        )
        .unwrap()
    }

    #[test]
    fn natural_completion_survives_snapshot_and_unblocks_zero_mode_without_input() {
        let mut state = machine();
        begin(&mut state, "mask.pb3".into(), 28, 1500, true, false).unwrap();
        state.write_interpreter_word(1452, 1);
        push_word(&mut state, 0).unwrap();
        let cursor = state.stack_cursor_bytes;
        command(&mut state, 555);
        assert!(state.dispatch_stopped);
        assert_eq!(state.stack_cursor_bytes, cursor);
        advance(&mut state, 1499);
        command(&mut state, 556);
        assert_eq!(state.interpreter_words[&81220], 1);
        let bytes = postcard::to_allocvec(&state).unwrap();
        let mut restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
        validate_cmvs390_vm_state(&restored).unwrap();
        advance(&mut restored, 1);
        assert!(restored.texture_presentation.is_none());
        command(&mut restored, 555);
        assert!(!restored.dispatch_stopped);
        assert_eq!(restored.stack_cursor_bytes, cursor - 4);
        assert_eq!(
            restored.texture_parents[&28].resource.as_deref(),
            Some("image28.pb3")
        );
        command(&mut restored, 556);
        assert_eq!(restored.interpreter_words[&81220], u32::MAX);
    }

    #[test]
    fn release_edge_finishes_hide_and_removes_only_its_parent() {
        let mut state = machine();
        begin(&mut state, "mask.pb3".into(), 28, 1000, false, false).unwrap();
        state.write_interpreter_word(1452, 1);
        state.input_advance_press = true;
        state.input_advance_release = true;
        push_word(&mut state, 1).unwrap();
        assert_eq!(
            command(&mut state, 555),
            Some(CmvsPs2aVmAction::RemoveTextureParentSurface { parent_slot: 28 })
        );
        assert!(state.texture_presentation.is_none());
        assert!(!state.texture_parents.contains_key(&28));
        assert!(!state.slot_objects.contains_key(&(1924 << 8 | 28)));
        assert!(state.texture_parents.contains_key(&7));
        assert!(!state.input_advance_release && !state.input_advance_press);
        assert_eq!(state.interpreter_words[&81220], 1);
    }

    #[test]
    fn replacing_a_hide_restores_its_image_and_clear_background_is_released() {
        let mut state = machine();
        begin(&mut state, "mask.pb3".into(), 28, 1000, false, false).unwrap();
        advance(&mut state, 500);
        for word in [750, 1500, 0] {
            push_word(&mut state, word).unwrap();
        }
        command(&mut state, 558);
        assert_eq!(
            state.texture_parents[&28].resource.as_deref(),
            Some("image28.pb3")
        );
        let bytes = postcard::to_allocvec(&state).unwrap();
        state = postcard::from_bytes(&bytes).unwrap();
        validate_cmvs390_vm_state(&state).unwrap();
        assert_eq!(
            state.texture_presentation.as_ref().unwrap().remaining_ms(),
            1500
        );
        assert_eq!(
            command(&mut state, 557),
            Some(CmvsPs2aVmAction::RemoveTextureParentSurface { parent_slot: 0 })
        );
        assert!(!state.texture_parents.contains_key(&0));
        assert!(state.texture_parents.contains_key(&28));
    }

    #[test]
    fn reveal_threshold_has_native_endpoints_and_spatial_midpoint() {
        let mut presentation = TexturePresentation {
            parent: 28,
            mask: "mask".into(),
            duration_ms: 1000,
            elapsed_ms: 0,
            showing: true,
        };
        assert!(presentation.alpha_lut().into_iter().all(|alpha| alpha == 0));
        presentation.elapsed_ms = 500;
        let alpha = presentation.alpha_lut();
        assert_eq!([alpha[0], alpha[128], alpha[255]], [255, 127, 0]);
        presentation.elapsed_ms = 1000;
        assert!(
            presentation
                .alpha_lut()
                .into_iter()
                .all(|alpha| alpha == 255)
        );
        presentation.showing = false;
        assert!(presentation.alpha_lut().into_iter().all(|alpha| alpha == 0));
        presentation.duration_ms = 0;
        presentation.elapsed_ms = 0;
        presentation.showing = true;
        assert!(
            presentation
                .alpha_lut()
                .into_iter()
                .all(|alpha| alpha == 255)
        );
    }
}
