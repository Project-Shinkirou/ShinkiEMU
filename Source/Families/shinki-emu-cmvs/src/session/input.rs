use super::*;

/// Physical buttons are distinct; releasing Space must not release held Enter.
#[derive(Default)]
pub(super) struct InputState {
    confirm: u8,
    cancel: u8,
    up_held: bool,
    down_held: bool,
    up_prev: bool,
    down_prev: bool,
    /// Whether a cancel press opened the system menu and no confirm has
    /// dispatched from it yet. The record-29 resume is the save/load
    /// orchestrator's contract, not the story dialogue path's: story
    /// chapters also render ch60 child presenters, so arming on every
    /// dialog confirm derailed the keyboard marathon (124 -> 7 loads).
    pub(super) menu_opened: bool,
    pub(super) menu_just_opened: bool,
    /// True once this menu session's save transaction has completed: the
    /// confirm dialog is torn down and the menu window's quad layer owns
    /// the corners again (the load-page arrow). Gates the quad-dispatch
    /// path so the save-phase clicks keep the presenter route.
    pub(super) menu_save_completed: bool,
    /// True until the first post-save confirm has dispatched the load
    /// body once; later confirms leave the selection unarmed so the
    /// record-park resume (not a fresh dispatcher dispatch) carries the
    /// load tail.
    pub(super) load_dispatch_fired: bool,
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
        self.menu_save_completed = false;
        self.load_dispatch_fired = false;
        self.menu_just_opened = false;
        self.menu_row = 1;
    }

    pub fn event(&mut self, vm: &mut CmvsPs2aVmState, event: &RuntimeEvent) {
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
            if tracing::enabled!(tracing::Level::INFO) && was_held && !vm.input_confirm_held {
                tracing::info!(
                    event = "cmvs.session.latch_armed",
                    tick = vm.tick,
                    frame = vm.current_frame
                );
            }
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
                // The load page's confirm key maps to key id 5
                // (`0x4604C2`: counter 0x940 == 1 -> id 5): the item
                // dispatcher's jve5 branch (`intproc` 187308) runs the
                // load body (194766). Publish the request and arm the
                // selection before any focus gating.
                if self.menu_save_completed && !self.load_dispatch_fired {
                    self.load_dispatch_fired = true;
                    tracing::info!(
                        event = "cmvs.session.load_confirm_armed",
                        tick = vm.tick,
                        menu_save_completed = self.menu_save_completed
                    );
                    vm.process_indexed_words.insert(66, 1);
                    // The load body's deep gate (`intproc` 195098) reads
                    // process word 2203 — an engine-side publish with no
                    // script writer — before the render-and-execute
                    // block. Publish it with the request.
                    vm.process_indexed_words.insert(2203, 1);
                    // The load body's data4 gate (`intproc` 195074):
                    // another engine-published context word (no script
                    // writer). Publish it into the menu script frame's
                    // data segment.
                    // `intproc` 187032 copies cmd 404's confirmed word
                    // (sysreg 5, byte 81240) into data word 4 on every
                    // dispatcher poll; the deep gate reads the copy. Arm
                    // the confirmed word instead of the data word so the
                    // next poll's copy carries it.
                    vm.interpreter_words.insert(81240, 1);
                    // `intproc` 195208 gates the load chain's tail on
                    // the save-request flag (process word 16) still
                    // being armed; the key confirm path never set it.
                    vm.process_indexed_words.insert(16, 1);
                    vm.input_selection_mirror = 5;
                    vm.input_selection_override = Some(5);
                    vm.load_confirm_selection = Some(5);
                    // The save flow parks the record-17 poll family; the
                    // item dispatcher only runs through the queued
                    // dispatch in the disabled-scheduler mode. Queue the
                    // dispatcher entry so the armed id reaches the ladder
                    // on the next rebuild.
                    vm.queue_dialog_dispatch(186_920, 1, 5);
                }
                // Story (non-menu) confirm: the item dispatcher only
                // runs through a queued dispatch; the story dialog's
                // advance is the jve1 confirmation chain, so queue the
                // dispatcher with the confirm id like the native
                // callback's message.
                if !self.menu_opened && vm.story_dispatch_armed {
                    vm.input_selection_mirror = 1;
                    vm.queue_dialog_dispatch(186_920, 1, 1);
                }
                let dialog_focus = self.menu_opened
                    && !vm.input_synthetic
                    && vm
                        .channel_child_presenters
                        .keys()
                        .any(|(ch, child)| *ch == 60 && *child >= 0);
                if dialog_focus {
                    // Each physical menu confirm pushes the record again
                    // (native callback behavior); the synthetic pump is
                    // excluded so the ring never piles overlays.
                    let slot = vm.process_indexed_words.get(&2).copied().unwrap_or(0);
                    vm.interpreter_words.insert(0x36E8, slot);
                    vm.interpreter_words.insert(0x36EC, slot);
                    vm.interpreter_words.insert(0x36F0, u32::MAX);
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
                    vm.interpreter_words.insert(3500 * 4, 1);
                    vm.interpreter_words.insert(3501 * 4, 1);
                    vm.interpreter_words.insert(3502 * 4, 179_768);
                    vm.interpreter_words.insert(0x29DC, 0);
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
                    if !self.menu_opened {
                        // The story context must be captured before the
                        // menu's own flow lays records over the table.
                        self.menu_just_opened = true;
                    }
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
        vm.menu_capture_story = self.menu_just_opened;
        self.menu_just_opened = false;
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

    /// Whether any confirm-group button is currently held.
    pub fn confirm_held(&self) -> bool {
        self.confirm != 0
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
}
