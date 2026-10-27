use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) enum ReloadMode {
    Pending,
    Failed,
    Fast,
    Composite,
}

/// The two native transition flags share one unsigned millisecond clock.
/// Failed loads keep their detached owner until release/replacement/destruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TextureReload {
    pub mode: ReloadMode,
    pub duration_ms: u32,
    pub elapsed_ms: u32,
    pub previous_identity: u64,
    pub previous_parent: CmvsTextureParentState,
    pub previous_children: BTreeMap<u16, CmvsTextureChildState>,
    pub previous_uri: Option<String>,
    pub current_uri: Option<String>,
    pub previous_child_uris: BTreeMap<u16, String>,
}

impl TextureReload {
    pub fn busy(&self) -> bool {
        matches!(self.mode, ReloadMode::Fast | ReloadMode::Composite)
    }

    pub fn alpha_word(&self) -> u32 {
        if self.duration_ms == 0 {
            return 256;
        }
        self.elapsed_ms.wrapping_shl(8) / self.duration_ms
    }
}

pub(crate) fn begin(
    state: &mut CmvsPs2aVmState,
    parent: u8,
    duration_ms: u32,
) -> Result<(), CoreError> {
    let key = super::runtime::slot_object_key(1924, u32::from(parent))?;
    let previous_identity = state.slot_objects.get(&key).copied().ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_VM_TEXTURE",
            "image reload has no live object identity",
        )
    })?;
    let identity = state.next_slot_object_id;
    state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
            "CMVS slot object identity space overflowed",
        )
    })?;
    state.slot_object_seed_words.remove(&key);
    let previous_parent = state
        .texture_parents
        .insert(parent, CmvsTextureParentState::default())
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_TEXTURE",
                "live image parent has no specialized state",
            )
        })?;
    let previous_children = state.texture_children.remove(&parent).unwrap_or_default();
    state.slot_objects.insert(key, identity);
    state.texture_children.insert(parent, BTreeMap::new());
    // 444BE0 first completes any earlier swap: the next owner is the last
    // loaded image, never a capture of the partially mixed framebuffer.
    state.texture_reloads.insert(
        parent,
        TextureReload {
            mode: ReloadMode::Pending,
            duration_ms,
            elapsed_ms: 0,
            previous_identity,
            previous_parent,
            previous_children,
            previous_uri: None,
            current_uri: None,
            previous_child_uris: BTreeMap::new(),
        },
    );
    Ok(())
}

pub(crate) fn advance_owner(
    current: &CmvsTextureParentState,
    reload: &mut TextureReload,
    delta_ms: u32,
) -> bool {
    if !reload.busy() {
        return true;
    }
    reload.elapsed_ms = reload.elapsed_ms.wrapping_add(delta_ms);
    if reload.elapsed_ms >= reload.duration_ms {
        return false;
    }
    //4448A0 copies only basic DWORD8..13; shape and reference stay detached.
    if reload.mode == ReloadMode::Composite {
        reload.previous_parent.auxiliary_pair_words = current.auxiliary_pair_words;
        reload.previous_parent.auxiliary_word = current.auxiliary_word;
        reload.previous_parent.float_position_words = current.float_position_words;
    }
    true
}

pub(crate) fn advance(state: &mut CmvsPs2aVmState, delta_ms: u32) {
    let parents = &state.texture_parents;
    state.texture_reloads.retain(|parent, reload| {
        parents
            .get(parent)
            .is_some_and(|current| advance_owner(current, reload, delta_ms))
    });
}

pub(crate) fn validate_owner(reload: &TextureReload) -> Result<(), CoreError> {
    if reload.mode == ReloadMode::Pending
        || (reload.busy() && reload.current_uri.is_none())
        || (reload.busy() && (reload.duration_ms == 0 || reload.elapsed_ms >= reload.duration_ms))
        || reload
            .previous_child_uris
            .keys()
            .any(|child| !reload.previous_children.contains_key(child))
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_RELOAD_STATE",
            "image reload owner or clock is invalid",
        ));
    }
    Ok(())
}

pub(crate) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    for (parent, reload) in &state.texture_reloads {
        if !state.texture_parents.contains_key(parent) {
            return Err(invalid(
                "SHINKI_EMU_CMVS_RELOAD_STATE",
                "image reload owner or clock is invalid",
            ));
        }
        validate_owner(reload)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occupied() -> CmvsPs2aVmState {
        let mut state = CmvsPs2aVmState::new(0);
        state.slot_objects.insert(1924 << 8 | 28, 7);
        state.next_slot_object_id = 8;
        state.texture_parents.insert(
            28,
            CmvsTextureParentState {
                surface_initialized: true,
                color_filter: Some([2, 0x102030, 0]),
                ..Default::default()
            },
        );
        state
    }

    #[test]
    fn replacement_keeps_new_identity_failed_owner_and_unsigned_clock() {
        let mut state = occupied();
        begin(&mut state, 28, u32::MAX).unwrap();
        let identity = state.slot_objects[&(1924 << 8 | 28)];
        let reload = state.texture_reloads.get_mut(&28).unwrap();
        assert_eq!(reload.previous_parent.color_filter, Some([2, 0x102030, 0]));
        reload.mode = ReloadMode::Failed;
        assert!(!reload.busy());
        advance(&mut state, 100);
        assert_eq!(state.slot_objects[&(1924 << 8 | 28)], identity);
        assert!(state.texture_reloads.contains_key(&28));
        begin(&mut state, 28, u32::MAX).unwrap();
        state.texture_reloads.get_mut(&28).unwrap().mode = ReloadMode::Fast;
        state.texture_reloads.get_mut(&28).unwrap().current_uri =
            Some("cmvs:/fixture/new.pb3".into());
        advance(&mut state, 50);
        let bytes = postcard::to_allocvec(&state).unwrap();
        let mut restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
        validate(&restored).unwrap();
        assert_eq!(restored.texture_reloads[&28].elapsed_ms, 50);
        advance(&mut restored, u32::MAX - 50);
        assert!(!restored.texture_reloads.contains_key(&28));
    }

    fn command(state: &mut CmvsPs2aVmState, id: u16, words: &[u32]) -> Option<CmvsPs2aVmAction> {
        for word in words.iter().rev() {
            super::super::stack::push_word(state, *word).unwrap();
        }
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
    fn commands_query_and_cancel_the_parent_owner_without_clobbering_missing_slot_results() {
        let mut state = occupied();
        state
            .frame_string_pool_bytes
            .insert(0, b"image.pb3\0".to_vec());
        command(&mut state, 708, &[28, 0, u32::MAX, 1]);
        assert_eq!(state.interpreter_words[&81220], 0);
        state.texture_reloads.get_mut(&28).unwrap().mode = ReloadMode::Fast;
        command(&mut state, 710, &[28]);
        assert_eq!(state.interpreter_words[&81220], 1);
        command(&mut state, 709, &[28]);
        assert!(!state.texture_reloads.contains_key(&28));
        command(&mut state, 710, &[28]);
        assert_eq!(state.interpreter_words[&81220], 0);
        state.write_interpreter_word(81220, 73);
        command(&mut state, 710, &[27]);
        assert_eq!(state.interpreter_words[&81220], 73);
        assert_eq!(state.interpreter_flag_words[&10532] & 0x10000, 0x10000);
    }
}
