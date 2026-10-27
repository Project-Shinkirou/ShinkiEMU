use super::*;

/// Physical buttons are distinct; releasing Space must not release held Enter.
#[derive(Default)]
pub(super) struct InputState {
    confirm: u8,
    cancel: u8,
    shift: u8,
    control: u8,
    up_held: bool,
    down_held: bool,
    up_prev: bool,
    down_prev: bool,
    pub(super) pointer_present: bool,
    /// Whether a cancel press opened the system menu and no confirm has
    /// dispatched from it yet. The record-29 resume is the save/load
    /// orchestrator's contract, not the story dialogue path's: story
    /// chapters also render ch60 child presenters, so arming on every
    /// dialog confirm derailed the keyboard marathon (124 -> 7 loads).
    pub(super) menu_opened: bool,
    /// The system menu's selected row while the menu session is open.
    /// The native confirm callback carries the row as the dispatch
    /// identity (frame local 8); intproc 70572 copies it into the
    /// menu-state word pw7, and rows 2/3 select the save screens.
    menu_row: u32,
}
impl InputState {
    /// Closes the system menu's session state: the native program
    /// switch (load) replaces the running program, so the menu's input
    /// routing, save/load arms and one-shot flags all reset.
    pub(super) fn close_menu(&mut self) {
        self.menu_opened = false;
        self.menu_row = 1;
    }

    pub fn event(&mut self, vm: &mut CmvsPs2aVmState, event: &RuntimeEvent) {
        if matches!(
            event,
            RuntimeEvent::PointerMove { .. } | RuntimeEvent::PointerButton { .. }
        ) {
            self.pointer_present = true;
        }
        vm.native_pointer_present = self.pointer_present;
        if let RuntimeEvent::Key { code, state, .. } = event {
            let modifier = match code {
                KeyCode::ShiftLeft => Some((&mut self.shift, 1)),
                KeyCode::ShiftRight => Some((&mut self.shift, 2)),
                KeyCode::ControlLeft => Some((&mut self.control, 1)),
                KeyCode::ControlRight => Some((&mut self.control, 2)),
                _ => None,
            };
            if let Some((keys, bit)) = modifier {
                match state {
                    KeyState::Pressed => *keys |= bit,
                    KeyState::Released => *keys &= !bit,
                    KeyState::Repeated => {}
                }
            }
        }
        vm.input_shift_held = self.shift != 0;
        vm.input_control_held = self.control != 0;
        let binding = match event {
            RuntimeEvent::Key {
                code: KeyCode::Enter,
                state,
                ..
            } => Some((1, state)),
            RuntimeEvent::Key {
                code: KeyCode::Space,
                state,
                ..
            } => Some((2, state)),
            RuntimeEvent::Key {
                code: KeyCode::NumpadEnter,
                state,
                ..
            } => Some((4, state)),
            RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state,
            } => Some((8, state)),
            _ => None,
        };
        if let Some((bit, state)) = binding {
            let was_held = self.confirm != 0;
            match state {
                KeyState::Pressed => self.confirm |= bit,
                KeyState::Released => self.confirm &= !bit,
                KeyState::Repeated => return,
            }
            vm.input_confirm_held = self.confirm != 0;
            vm.input_advance_press |= !was_held && vm.input_confirm_held;
            vm.input_advance_release |= was_held && !vm.input_confirm_held;
            // Native key confirm on a focused dialog: the load/save
            // orchestrator (`sub_471F50` at 0x472883) writes the engine
            // trio [+0x36E8]=slot, [+0x36EC]=slot, [+0x36F0]=-1 and then
            // pushes record slot 29 through `sub_477F60`. Mirror that
            // contract so the record-29 handler (intproc 369576) resumes
            // with its parameters in the engine trio.
            if was_held && !vm.input_confirm_held {
                // The load page's confirm publishes the load request
                // through process word 66 before the record-29 resume:
                // the orchestrator's load branch reads it (the load body
                // hard-codes slot 860 itself). Post-save menu = the load
                // page.
                let dialog_focus = self.menu_opened
                    && vm.texture_children.get(&60).is_some_and(|children| {
                        children.values().any(|child| child.surface_initialized)
                    });
                if dialog_focus {
                    // Each menu confirm pushes the record again (native
                    // callback behavior).
                    let slot = vm.process_indexed_words.get(&2).copied().unwrap_or(0);
                    vm.write_interpreter_word(0x36E8, slot);
                    vm.write_interpreter_word(0x36EC, slot);
                    vm.write_interpreter_word(0x36F0, u32::MAX);
                    // The native callback pushes record slot 29
                    // (`sub_477F60`), whose table row resumes at intproc
                    // 369576 — the save/load orchestrator's handler. The
                    // 25366 dispatcher entry is the story dialog's ring,
                    // not the menu record path.
                    vm.forced_dialog_record = Some((1, 369_576, self.menu_row.max(1)));
                    // The native menu's confirm consumes the filter-chain
                    // selection (the click commit): the save-screen poll
                    // reads the cleared chain as -2 and takes its exit.
                    vm.filter_chain_active_records.clear();
                    // The Win32 Enter handler posts the same engine resume
                    // request the click handler does: the save sequence's
                    // chained waits (193138+) each need one.
                    vm.engine_request_armed = true;
                    vm.write_interpreter_word(3500 * 4, 1);
                    vm.write_interpreter_word(3501 * 4, 1);
                    vm.write_interpreter_word(3502 * 4, 179_768);
                    vm.write_interpreter_word(0x29DC, 0);
                    // The record chain's tail lands in `Save_writeFile`
                    // (cmd694) for the selected row; the record-6 resume
                    // phase that reaches it is still being recovered, so
                    // the session bridges the final write directly.
                    // The はい lands on save first; a later confirm on a
                    // row that already holds a file loads it back
                    // (`Prog_loadAndSwitch`), matching the menu's
                    // save-then-load walkthrough.
                    // The save and load both run through their native
                    // script commands (cmd 694 / cmd 693) once the menu
                    // chain publishes the request; no host-side slot
                    // bridging remains.
                }
            }
            // The native engine's input branch consumes a click while a
            // dialogue line is waiting: the main tick clears the "line
            // shown" latch (process word 9, set at intproc 70620 when the
            // text surface is configured) so the pump loop resumes. The
            // headless host bridges that click directly.
            if !was_held
                && vm.input_confirm_held
                && vm.process_indexed_words.get(&9).copied().unwrap_or(0) != 0
            {
                vm.process_indexed_words.insert(9, 0);
            }
            // Native auto mode is pumped by the session: it synthesizes a
            // click through this same path when the line-wait latch arms,
            // so the coroutine-resume chain runs exactly as for physical
            // clicks.
        }
        // The cancel group mirrors the recovered manager latch
        // (`sub_45D2D0` reads dword 296): held state, no edge semantics.
        let cancel_binding = match event {
            RuntimeEvent::Key {
                code: KeyCode::Escape,
                state,
                ..
            }
            | RuntimeEvent::PointerButton {
                button: PointerButton::Secondary,
                state,
            } => Some((1u8, state)),
            _ => None,
        };
        if let Some((bit, state)) = cancel_binding {
            match state {
                KeyState::Pressed => {
                    self.cancel |= bit;
                    self.menu_opened = true;
                    self.menu_row = 1;
                }
                KeyState::Released => self.cancel &= !bit,
                KeyState::Repeated => return,
            }
            vm.input_cancel_held = self.cancel != 0;
        }
        match event {
            RuntimeEvent::Key {
                code: KeyCode::ArrowUp,
                state,
                ..
            } => self.up_held = *state == KeyState::Pressed,
            RuntimeEvent::Key {
                code: KeyCode::ArrowDown,
                state,
                ..
            } => self.down_held = *state == KeyState::Pressed,
            _ => {}
        }
        vm.input_up_edge = self.up_held && !self.up_prev;
        vm.input_down_edge = self.down_held && !self.down_prev;
        vm.menu_session_live = self.menu_opened;
        if self.menu_opened {
            if vm.input_down_edge {
                self.menu_row = self.menu_row.saturating_add(1).min(9);
            }
            if vm.input_up_edge {
                self.menu_row = self.menu_row.saturating_sub(1).max(1);
            }
        }
        self.up_prev = self.up_held;
        self.down_prev = self.down_held;
        if matches!(
            event,
            RuntimeEvent::WindowFocused { focused: false }
                | RuntimeEvent::WindowSuspended { suspended: true }
        ) {
            self.pointer_present = false;
            vm.native_pointer_present = false;
            self.shift = 0;
            self.control = 0;
            vm.input_shift_held = false;
            vm.input_control_held = false;
            self.confirm = 0;
            self.cancel = 0;
            self.up_held = false;
            self.down_held = false;
            self.up_prev = false;
            self.down_prev = false;
            vm.input_confirm_held = false;
            vm.input_cancel_held = false;
            // Focus loss is cancellation, never a synthetic story advance.
            vm.input_advance_press = false;
            vm.input_advance_release = false;
        }
    }

    /// Native 460520 rebuilds logical held inputs before every dispatch pass.
    /// Restoring a VM must not resurrect keys no longer physically held.
    pub(super) fn project(&self, vm: &mut CmvsPs2aVmState) {
        vm.input_confirm_held = self.confirm != 0;
        vm.input_cancel_held = self.cancel != 0;
        vm.input_shift_held = self.shift != 0;
        vm.input_control_held = self.control != 0;
        vm.native_pointer_present = self.pointer_present;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode, state: KeyState) -> RuntimeEvent {
        RuntimeEvent::Key {
            code,
            state,
            modifiers: KeyModifiers {
                shift: false,
                control: false,
                alt: false,
                super_key: false,
            },
        }
    }
    #[test]
    fn release_between_ticks_survives_until_native_consume() {
        let mut input = InputState::default();
        let mut vm = CmvsPs2aVmState::new(0);
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Released));
        assert!(vm.input_advance_release);
        assert!(!vm.input_confirm_held);
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Repeated));
        assert!(vm.input_advance_release);
    }
    #[test]
    fn independent_buttons_and_focus_loss_do_not_advance() {
        let mut input = InputState::default();
        let mut vm = CmvsPs2aVmState::new(0);
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::Space, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::Space, KeyState::Released));
        assert!(vm.input_confirm_held);
        assert!(!vm.input_advance_release);
        input.event(&mut vm, &RuntimeEvent::WindowFocused { focused: false });
        assert!(!vm.input_confirm_held);
        assert!(!vm.input_advance_release);
        assert!(!vm.input_advance_press);
    }
    #[test]
    fn cancel_and_directional_edges_rebuild_per_event() {
        let mut input = InputState::default();
        let mut vm = CmvsPs2aVmState::new(0);
        input.event(&mut vm, &key(KeyCode::Escape, KeyState::Pressed));
        assert!(vm.input_cancel_held);
        input.event(&mut vm, &key(KeyCode::ArrowDown, KeyState::Pressed));
        assert!(vm.input_down_edge);
        assert!(!vm.input_up_edge);
        input.event(&mut vm, &key(KeyCode::ArrowDown, KeyState::Released));
        input.event(&mut vm, &key(KeyCode::ArrowDown, KeyState::Pressed));
        assert!(vm.input_down_edge);
        input.event(&mut vm, &key(KeyCode::Escape, KeyState::Released));
        assert!(!vm.input_cancel_held);
    }

    #[test]
    fn modifiers_are_independent_native_skip_inputs_not_confirmation() {
        let mut input = InputState::default();
        let mut vm = CmvsPs2aVmState::new(0);
        let read_skip = |vm: &mut CmvsPs2aVmState| {
            execute_cmvs390_frame(
                vm,
                &crate::CmvsPs2aInstructionFrame::Command {
                    span: crate::CmvsPs2aSourceSpan {
                        offset: 0,
                        byte_length: 2,
                    },
                    command_id: 751,
                },
            )
            .unwrap();
            vm.interpreter_words[&81220]
        };
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Pressed));
        assert_eq!(read_skip(&mut vm), 0);
        input.event(&mut vm, &key(KeyCode::ShiftLeft, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::ShiftRight, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::ShiftLeft, KeyState::Released));
        assert_eq!(read_skip(&mut vm), 1);
        input.event(&mut vm, &key(KeyCode::ShiftRight, KeyState::Released));
        assert_eq!(read_skip(&mut vm), 0);
        input.event(&mut vm, &key(KeyCode::ControlLeft, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::ControlRight, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::ControlLeft, KeyState::Released));
        assert_eq!(read_skip(&mut vm), 1);
        input.event(&mut vm, &RuntimeEvent::WindowFocused { focused: false });
        assert_eq!(read_skip(&mut vm), 0);
        assert!(!vm.input_confirm_held);
    }

    #[test]
    fn restored_skip_state_rebinds_to_current_physical_keys() {
        let mut input = InputState::default();
        let mut vm = CmvsPs2aVmState::new(0);
        input.event(&mut vm, &key(KeyCode::ControlLeft, KeyState::Pressed));
        let bytes = postcard::to_allocvec(&vm).unwrap();
        let mut restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
        input.event(&mut vm, &key(KeyCode::ControlLeft, KeyState::Released));
        input.project(&mut restored);
        execute_cmvs390_frame(
            &mut restored,
            &crate::CmvsPs2aInstructionFrame::Command {
                span: crate::CmvsPs2aSourceSpan {
                    offset: 0,
                    byte_length: 2,
                },
                command_id: 751,
            },
        )
        .unwrap();
        assert_eq!(restored.interpreter_words[&81220], 0);
    }
}
