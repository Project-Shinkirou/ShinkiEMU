use super::*;

fn command(
    state: &mut CmvsPs2aVmState,
    id: u16,
    top_first: &[u32],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    for word in top_first.iter().rev() {
        push_word(state, *word)?;
    }
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: crate::CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: id,
        },
    )
}

fn metadata() -> crate::cmv::CmvMetadata {
    crate::cmv::CmvMetadata {
        width: 16,
        height: 16,
        frame_count: 6,
        rate_num: 100000,
        rate_den: 10000,
        has_alpha: false,
    }
}

fn fixture(start: u32) -> CmvsPs2aVmState {
    let mut state = CmvsPs2aVmState::new(0);
    state
        .frame_string_pool_bytes
        .insert(0, b"fixture.cmv\0".to_vec());
    let action = command(&mut state, 776, &[0, 7, 0, 0xdeadbeef, start]).unwrap();
    assert!(matches!(action, Some(CmvsPs2aVmAction::OpenCmvResource {
        controller_slot: 0, parent_slot: 7, start_frame, ..
    }) if start_frame == start));
    assert!(!state.texture_parents[&7].surface_initialized);
    opened(&mut state, 0, metadata()).unwrap();
    state
}

fn play(state: &mut CmvsPs2aVmState, range: u32, start: u32, end: u32, repeat: bool) {
    assert!(
        command(state, 777, &[0, range, start, end, u32::from(repeat)])
            .unwrap()
            .is_none()
    );
}

fn query(state: &mut CmvsPs2aVmState, id: u16) -> u32 {
    command(state, id, &[0]).unwrap();
    state.interpreter_words[&81220]
}

#[test]
fn open_pops_five_words_and_777_preserves_the_parent_identity() {
    let mut state = fixture(2);
    assert_eq!(state.stack_cursor_bytes, 0);
    let identity = state.slot_objects[&slot_object_key(PARENT_TABLE, 7).unwrap()];
    push_word(&mut state, 0x1234).unwrap();
    play(&mut state, 0, 2, u32::MAX, false);
    assert_eq!(state.stack_cursor_bytes, 4);
    assert_eq!(
        state.slot_objects[&slot_object_key(PARENT_TABLE, 7).unwrap()],
        identity
    );
    assert_eq!(query(&mut state, 798), 2);
    assert_eq!(query(&mut state, 794), 1);
    assert!(advance(&mut state, 99).is_empty());
    assert_eq!(
        advance(&mut state, 1),
        vec![CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 3
        }]
    );
}

#[test]
fn rational_clock_retains_subframe_time_and_finishes_after_inclusive_end() {
    let mut state = fixture(0);
    state.cmv_controllers.get_mut(&0).unwrap().rate_num = 150000;
    play(&mut state, 0, 0, 2, false);
    assert!(advance(&mut state, 66).is_empty());
    assert_eq!(
        advance(&mut state, 1),
        vec![CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 1
        }]
    );
    advance(&mut state, 67);
    assert_eq!(query(&mut state, 798), 2);
    assert_eq!(query(&mut state, 794), 1);
    assert!(advance(&mut state, 66).is_empty());
    assert_eq!(query(&mut state, 794), 0);
    assert_eq!(query(&mut state, 783), 0);
    assert_eq!(state.interpreter_words[&81236], 2);
}

#[test]
fn repeat_uses_inclusive_modulo_and_only_emits_changed_frames() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 2, true);
    assert!(advance(&mut state, 600).is_empty());
    assert_eq!(query(&mut state, 798), 0);
    assert_eq!(
        advance(&mut state, 1000),
        vec![CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 1
        }]
    );
    assert_eq!(query(&mut state, 794), 1);
}

#[test]
fn queued_range_switches_at_boundary_or_explicit_apply() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 2, true);
    play(&mut state, 1, 4, 5, false);
    assert_eq!(
        advance(&mut state, 300),
        vec![CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 4
        }]
    );
    assert_eq!(query(&mut state, 783), 1);
    assert_eq!(state.interpreter_words[&81236], 4);
    assert_eq!(query(&mut state, 796), 0);
    play(&mut state, 2, 1, 3, true);
    assert_eq!(
        command(&mut state, 796, &[0]).unwrap(),
        Some(CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 1
        })
    );
    assert_eq!(state.interpreter_words[&81220], 1);
    assert_eq!(query(&mut state, 796), 0);
    assert_eq!(query(&mut state, 783), 2);
}

#[test]
fn pause_resume_resets_fraction_and_end_reentry_requires_resume() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 1, false);
    advance(&mut state, 90);
    command(&mut state, 780, &[0]).unwrap();
    assert!(advance(&mut state, 1000).is_empty());
    assert_eq!(query(&mut state, 794), 1);
    command(&mut state, 779, &[0]).unwrap();
    assert!(advance(&mut state, 10).is_empty());
    advance(&mut state, 190);
    assert_eq!(query(&mut state, 794), 0);
    play(&mut state, 0, 0, 2, false);
    assert!(advance(&mut state, 100).is_empty());
    command(&mut state, 779, &[0]).unwrap();
    advance(&mut state, 100);
    assert_eq!(query(&mut state, 798), 2);
}

#[test]
fn stop_seek_and_mode_ten_stop_frame_have_native_query_results() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 5, false);
    command(&mut state, 781, &[0, 10]).unwrap();
    command(&mut state, 797, &[0, 2]).unwrap();
    advance(&mut state, 1000);
    assert_eq!(query(&mut state, 798), 2);
    assert_eq!(query(&mut state, 794), 1);
    assert_eq!(
        command(&mut state, 795, &[0, 4]).unwrap(),
        Some(CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 4
        })
    );
    assert_eq!(state.interpreter_words[&81220], 1);
    assert_eq!(query(&mut state, 794), 0);
    assert!(advance(&mut state, 1000).is_empty());
}

#[test]
fn mode_fifteen_uses_total_elapsed_modulo_stream_duration() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 5, false);
    command(&mut state, 781, &[0, 15]).unwrap();
    advance(&mut state, 1450);
    assert_eq!(query(&mut state, 798), 2);
    advance(&mut state, 50);
    assert_eq!(query(&mut state, 798), 3);
    assert_eq!(query(&mut state, 794), 1);
    state.cmv_controllers.get_mut(&0).unwrap().audio_clock_ms = Some(1300);
    advance(&mut state, 10);
    assert_eq!(query(&mut state, 798), 1);
}

#[test]
fn snapshot_resume_preserves_pending_ranges_fraction_and_source_pin() {
    let mut state = fixture(0);
    state.cmv_controllers.get_mut(&0).unwrap().source_uri =
        Some("cmvs:/archive/010/imgvideo/fixture.cmv".into());
    play(&mut state, 0, 0, 2, true);
    play(&mut state, 1, 4, 5, false);
    advance(&mut state, 250);
    let saved = serde_json::to_value(&state).unwrap();
    let mut restored: CmvsPs2aVmState = serde_json::from_value(saved.clone()).unwrap();
    opened(&mut restored, 0, metadata()).unwrap();
    assert_eq!(
        advance(&mut restored, 50),
        vec![CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 4
        }]
    );
    assert_eq!(query(&mut restored, 783), 1);
    assert_eq!(
        restored.cmv_controllers[&0].source_uri.as_deref(),
        Some("cmvs:/archive/010/imgvideo/fixture.cmv")
    );
    let mut old = saved;
    old["format_version"] = 16.into();
    assert!(serde_json::from_value::<CmvsPs2aVmState>(old).is_err());
}

#[test]
fn moved_parent_keeps_binding_replacement_parent_does_not_receive_frames() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 5, true);
    command(&mut state, 40, &[9, 7]).unwrap();
    assert_eq!(bound_target(&state, 0), Some(CmvTarget::Live(9)));
    assert_eq!(advance(&mut state, 100).len(), 1);
    command(&mut state, 32, &[9]).unwrap();
    assert_eq!(bound_target(&state, 0), None);
    assert!(advance(&mut state, 100).is_empty());
    assert_eq!(query(&mut state, 798), 2);
}

fn reload_cmv_parent(state: &mut CmvsPs2aVmState, mode: super::super::texture_reload::ReloadMode) {
    assert!(matches!(
        command(state, 708, &[7, 0, 300, 0]).unwrap(),
        Some(CmvsPs2aVmAction::ReloadTextureParentImage { parent_slot: 7, .. })
    ));
    let reload = state.texture_reloads.get_mut(&7).unwrap();
    reload.mode = mode;
    if reload.busy() {
        reload.current_uri = Some("cmvs:/fixture/replacement.pb3".into());
    }
}

#[test]
fn composite_reload_advances_the_retained_cmv_through_move_and_release() {
    use super::super::texture_reload::{self, ReloadMode};

    for cancel in [false, true] {
        let mut state = fixture(0);
        play(&mut state, 0, 0, 5, true);
        let original_identity = state.cmv_controllers[&0].parent_identity;
        state.texture_parents.get_mut(&7).unwrap().alpha = Some(77);
        reload_cmv_parent(&mut state, ReloadMode::Composite);
        let new_identity = state.slot_objects[&slot_object_key(PARENT_TABLE, 7).unwrap()];
        let new_parent = state.texture_parents[&7].clone();
        assert_ne!(new_identity, original_identity);
        assert_eq!(
            state.texture_reloads[&7].previous_identity,
            original_identity
        );
        assert_eq!(bound_target(&state, 0), Some(CmvTarget::ReloadPrevious(7)));
        assert_eq!(
            advance(&mut state, 100),
            vec![CmvsPs2aVmAction::PresentCmvFrame {
                controller_slot: 0,
                frame: 1
            }]
        );
        texture_reload::advance(&mut state, 100);
        assert_eq!(state.texture_reloads[&7].previous_parent.alpha, Some(77));
        assert_eq!(state.texture_parents[&7], new_parent);
        command(&mut state, 40, &[9, 7]).unwrap();
        assert_eq!(bound_target(&state, 0), Some(CmvTarget::ReloadPrevious(9)));
        texture_reload::advance(&mut state, 100);
        assert_eq!(
            advance(&mut state, 100),
            vec![CmvsPs2aVmAction::PresentCmvFrame {
                controller_slot: 0,
                frame: 2
            }]
        );
        assert_eq!(query(&mut state, 798), 2);
        assert_eq!(
            state.slot_objects[&slot_object_key(PARENT_TABLE, 9).unwrap()],
            new_identity
        );
        assert_eq!(state.texture_parents[&9], new_parent);
        if cancel {
            assert!(matches!(
                command(&mut state, 709, &[9]).unwrap(),
                Some(CmvsPs2aVmAction::ReleaseTextureReload { parent_slot: 9 })
            ));
        } else {
            texture_reload::advance(&mut state, 100);
        }
        assert_eq!(bound_target(&state, 0), None);
        assert!(!state.texture_reloads.contains_key(&9));
        assert!(advance(&mut state, 100).is_empty());
        // 441E10 is not called by image release: the controller's independent
        // clock/query state survives. No frame may alias the replacement.
        assert_eq!(query(&mut state, 798), 3);
        assert_eq!(query(&mut state, 794), 1);
        assert_eq!(state.cmv_controllers[&0].parent_identity, original_identity);
        assert_eq!(
            state.slot_objects[&slot_object_key(PARENT_TABLE, 9).unwrap()],
            new_identity
        );
        assert_eq!(state.texture_parents[&9], new_parent);
    }
}

#[test]
fn failed_reload_retains_cmv_but_fast_and_superseding_reload_do_not_alias_it() {
    use super::super::texture_reload::{self, ReloadMode};

    let mut state = fixture(0);
    play(&mut state, 0, 0, 5, true);
    reload_cmv_parent(&mut state, ReloadMode::Failed);
    assert_eq!(bound_target(&state, 0), Some(CmvTarget::ReloadPrevious(7)));
    texture_reload::advance(&mut state, 1000);
    assert_eq!(advance(&mut state, 100).len(), 1);
    assert_eq!(query(&mut state, 798), 1);
    reload_cmv_parent(&mut state, ReloadMode::Composite);
    assert_eq!(bound_target(&state, 0), None);
    assert!(advance(&mut state, 100).is_empty());
    assert_eq!(query(&mut state, 798), 2);

    let mut state = fixture(0);
    play(&mut state, 0, 0, 5, true);
    reload_cmv_parent(&mut state, ReloadMode::Fast);
    assert_eq!(bound_target(&state, 0), None);
    assert!(advance(&mut state, 100).is_empty());
    assert_eq!(query(&mut state, 798), 1);
    assert_eq!(query(&mut state, 794), 1);
}

#[test]
fn opening_new_cmv_destroys_active_reload_and_uses_a_fresh_root_identity() {
    use super::super::texture_reload::ReloadMode;

    let mut state = fixture(0);
    reload_cmv_parent(&mut state, ReloadMode::Composite);
    let replacement_identity = state.slot_objects[&slot_object_key(PARENT_TABLE, 7).unwrap()];
    assert!(matches!(
        command(&mut state, 776, &[0, 7, 0, 0, 0]).unwrap(),
        Some(CmvsPs2aVmAction::OpenCmvResource {
            controller_slot: 0,
            parent_slot: 7,
            ..
        })
    ));
    assert!(!state.texture_reloads.contains_key(&7));
    assert_ne!(
        state.cmv_controllers[&0].parent_identity,
        replacement_identity
    );
    assert_eq!(bound_target(&state, 0), Some(CmvTarget::Live(7)));
}

#[test]
fn destroy_keeps_parent_and_missing_controller_queries_follow_native_results() {
    let mut state = fixture(0);
    let identity = state.slot_objects[&slot_object_key(PARENT_TABLE, 7).unwrap()];
    assert_eq!(
        command(&mut state, 778, &[0]).unwrap(),
        Some(CmvsPs2aVmAction::ReleaseCmvResource { controller_slot: 0 })
    );
    assert_eq!(
        state.slot_objects[&slot_object_key(PARENT_TABLE, 7).unwrap()],
        identity
    );
    assert_eq!(query(&mut state, 783), u32::MAX);
    assert_eq!(state.interpreter_words[&81236], 0);
    for id in [794, 796, 798] {
        assert_eq!(query(&mut state, id), 0);
    }
    assert!(command(&mut state, 779, &[0]).unwrap().is_none());
    assert!(command(&mut state, 778, &[0]).unwrap().is_none());
}

#[test]
fn alternative_open_reads_sixth_word_and_slot_bounds_leave_results_unchanged() {
    let mut state = fixture(0);
    assert!(matches!(
        command(&mut state, 782, &[1, 8, 0, 123, 3, 3]).unwrap(),
        Some(CmvsPs2aVmAction::OpenCmvResource {
            controller_slot: 1,
            parent_slot: 8,
            start_frame: 3,
            ..
        })
    ));
    assert_eq!(state.cmv_controllers[&1].open_mode, 3);
    state.write_interpreter_word(81220, 99);
    command(&mut state, 783, &[3]).unwrap();
    assert_eq!(state.interpreter_words[&81220], 99);
    assert!(state.interpreter_flag_words[&10532] & 0x0100_0000 != 0);
    assert_eq!(state.cmv_controllers.len(), 2);
}

#[test]
fn invalid_range_is_explicit_and_494_is_not_controller_playback() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 5, false);
    command(&mut state, 494, &[7, u32::MAX]).unwrap();
    assert_eq!(state.interpreter_words[&81220], u32::MAX);
    assert_eq!(query(&mut state, 794), 1);
    assert!(command(&mut state, 777, &[0, 16, 0, 5, 0]).is_err());
    let mut state = fixture(0);
    assert!(command(&mut state, 777, &[0, 0, 0, 6, 0]).is_err());
}

#[test]
fn native_normal_clock_clamps_one_tick_to_one_second() {
    let mut state = fixture(0);
    play(&mut state, 0, 0, 5, true);
    advance(&mut state, 5500);
    assert_eq!(query(&mut state, 798), 4);
    assert_eq!(query(&mut state, 794), 1);
}

#[test]
fn successful_callback_rejects_invalid_initial_frame_and_changed_restore_metadata() {
    let mut state = fixture(0);
    let mut changed = metadata();
    changed.width = 32;
    assert!(opened(&mut state, 0, changed).is_err());
    assert!(!state.texture_parents[&7].surface_initialized);
    command(&mut state, 776, &[0, 7, 0, 0, 6]).unwrap();
    assert!(opened(&mut state, 0, metadata()).is_err());
    assert!(advance(&mut state, 1000).is_empty());
}

#[test]
fn shard_transfer_keeps_movie_identity_after_parent_reuse_and_table_clear() {
    let mut state = fixture(0);
    state.texture_parents.get_mut(&7).unwrap().image_info = [16, 16, 0];
    play(&mut state, 0, 0, 5, true);
    command(&mut state, 680, &[0, 7, 20, 0]).unwrap();
    assert_eq!(bound_target(&state, 0), Some(CmvTarget::SceneImage(0)));
    command(&mut state, 32, &[7]).unwrap();
    command(&mut state, 47, &[]).unwrap();
    assert_eq!(
        advance(&mut state, 100),
        vec![CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 1,
        }]
    );
    let encoded = postcard::to_allocvec(&state).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
    assert_eq!(bound_target(&restored, 0), Some(CmvTarget::SceneImage(0)));
    assert_eq!(
        advance(&mut restored, 100),
        vec![CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 2,
        }]
    );
    command(&mut restored, 672, &[0]).unwrap();
    assert_eq!(bound_target(&restored, 0), None);
    assert!(advance(&mut restored, 100).is_empty());
    assert_eq!(restored.cmv_controllers[&0].current_frame, 3);
}
