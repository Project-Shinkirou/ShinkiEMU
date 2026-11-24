use shinki_emu_sdk::runtime::{KeyCode, KeyState, PointerButton, RuntimeEvent};

#[derive(Clone, Copy, Eq, PartialEq)]
struct Binding {
    device: i32,
    code: u8,
}

pub(crate) struct Input {
    keys: [bool; 256],
    key_edges: [bool; 256],
    buttons: [bool; 3],
    button_edges: [bool; 3],
    pub(crate) pointer: [i32; 2],
    pub(crate) target: Option<i32>,
    bindings: [Vec<Binding>; 20],
    pub(crate) cursor_settings: (bool, i32, i32, i32),
    pub(crate) cursor_kind: Option<i32>,
    pub(crate) cursor_visible: bool,
}

impl Default for Input {
    fn default() -> Self {
        let mut input = Self {
            keys: [false; 256],
            key_edges: [false; 256],
            buttons: [false; 3],
            button_edges: [false; 3],
            pointer: [-1; 2],
            target: None,
            bindings: std::array::from_fn(|_| Vec::new()),
            cursor_settings: (false, 0, 0, 0),
            cursor_kind: None,
            cursor_visible: true,
        };
        for (action, keys) in [
            (0, &[0xc8][..]),
            (1, &[0xd0]),
            (2, &[0xcb]),
            (3, &[0xcd]),
            (4, &[0x1c, 0x9c]),
            (5, &[1]),
            (18, &[0xc9]),
            (19, &[0xd1]),
        ] {
            for &key in keys {
                input.change_binding(action, 0, key, true);
            }
        }
        input
    }
}

impl Input {
    pub(crate) fn event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::Key { code, state, .. } => {
                if let Some(code) = scancode(code) {
                    update(&mut self.keys[code], &mut self.key_edges[code], state);
                }
            }
            RuntimeEvent::PointerMove { x, y } => self.pointer = [x as i32, y as i32],
            RuntimeEvent::PointerButton { button, state } => {
                let index = match button {
                    PointerButton::Primary => 0,
                    PointerButton::Secondary => 1,
                    PointerButton::Middle => 2,
                    _ => return,
                };
                update(
                    &mut self.buttons[index],
                    &mut self.button_edges[index],
                    state,
                );
            }
            RuntimeEvent::WindowFocused { focused: false } => {
                self.keys.fill(false);
                self.buttons.fill(false);
                self.pointer = [-1; 2];
                self.next_tick();
            }
            _ => {}
        }
    }

    pub(crate) fn next_tick(&mut self) {
        self.key_edges.fill(false);
        self.button_edges.fill(false);
    }

    pub(crate) fn mouse(&self, button: usize, edge: bool) -> bool {
        let buttons = if edge {
            &self.button_edges
        } else {
            &self.buttons
        };
        buttons.get(button).copied().unwrap_or(false)
    }

    pub(crate) fn edge_direction(&self, width: u32, height: u32) -> i32 {
        let [x, y] = self.pointer;
        if x < 0 || y < 0 {
            0
        } else if y == 0 {
            8
        } else if y as u32 >= height.saturating_sub(1) {
            2
        } else if x == 0 {
            4
        } else if x as u32 >= width.saturating_sub(1) {
            6
        } else {
            0
        }
    }

    pub(crate) fn key(&self, scan: u8, edge: bool) -> bool {
        if edge {
            self.key_edges[usize::from(scan)]
        } else {
            self.keys[usize::from(scan)]
        }
    }

    pub(crate) fn action(&self, action: i32, edge: bool) -> i32 {
        let Some(bindings) = self.bindings.get(action as usize) else {
            return 0;
        };
        i32::from(bindings.iter().any(|binding| match binding.device {
            0 => self.key(binding.code, edge),
            1 => {
                let keys = if edge {
                    &self.button_edges
                } else {
                    &self.buttons
                };
                keys.get(usize::from(binding.code))
                    .copied()
                    .unwrap_or_default()
            }
            _ => false,
        }))
    }

    pub(crate) fn first_action(&self, start: i32, count: i32, edge: bool) -> i32 {
        let end = if count == 0 {
            self.bindings.len() as i32
        } else {
            start.saturating_add(count).min(self.bindings.len() as i32)
        };
        (start.max(0)..end)
            .find(|&action| self.action(action, edge) != 0)
            .unwrap_or(-1)
    }

    pub(crate) fn change_binding(&mut self, action: i32, device: i32, code: u8, add: bool) -> i32 {
        let Some(bindings) = self.bindings.get_mut(action as usize) else {
            return 1;
        };
        let binding = Binding { device, code };
        if !matches!(device, 0 | 1) {
            return 2;
        }
        if add {
            if !bindings.contains(&binding) {
                bindings.push(binding);
            }
        } else if let Some(index) = bindings.iter().position(|existing| *existing == binding) {
            bindings.remove(index);
        } else {
            return 3;
        }
        0
    }
}

fn update(held: &mut bool, edge: &mut bool, state: KeyState) {
    match state {
        KeyState::Pressed => {
            *edge |= !*held;
            *held = true;
        }
        KeyState::Released => *held = false,
        KeyState::Repeated => {}
    }
}

fn scancode(key: KeyCode) -> Option<usize> {
    use KeyCode::*;
    Some(match key {
        Escape => 0x01,
        Digit1 => 0x02,
        Digit2 => 0x03,
        Digit3 => 0x04,
        Digit4 => 0x05,
        Digit5 => 0x06,
        Digit6 => 0x07,
        Digit7 => 0x08,
        Digit8 => 0x09,
        Digit9 => 0x0a,
        Digit0 => 0x0b,
        Minus => 0x0c,
        Equals => 0x0d,
        Backspace => 0x0e,
        Tab => 0x0f,
        Q => 0x10,
        W => 0x11,
        E => 0x12,
        R => 0x13,
        T => 0x14,
        Y => 0x15,
        U => 0x16,
        I => 0x17,
        O => 0x18,
        P => 0x19,
        BracketLeft => 0x1a,
        BracketRight => 0x1b,
        Enter => 0x1c,
        ControlLeft => 0x1d,
        A => 0x1e,
        S => 0x1f,
        D => 0x20,
        F => 0x21,
        G => 0x22,
        H => 0x23,
        J => 0x24,
        K => 0x25,
        L => 0x26,
        Semicolon => 0x27,
        Apostrophe => 0x28,
        Grave => 0x29,
        ShiftLeft => 0x2a,
        Backslash => 0x2b,
        Z => 0x2c,
        X => 0x2d,
        C => 0x2e,
        V => 0x2f,
        B => 0x30,
        N => 0x31,
        M => 0x32,
        Comma => 0x33,
        Period => 0x34,
        Slash => 0x35,
        ShiftRight => 0x36,
        NumpadMultiply => 0x37,
        AltLeft => 0x38,
        Space => 0x39,
        F1 => 0x3b,
        F2 => 0x3c,
        F3 => 0x3d,
        F4 => 0x3e,
        F5 => 0x3f,
        F6 => 0x40,
        F7 => 0x41,
        F8 => 0x42,
        F9 => 0x43,
        F10 => 0x44,
        Numpad7 => 0x47,
        Numpad8 => 0x48,
        Numpad9 => 0x49,
        NumpadSubtract => 0x4a,
        Numpad4 => 0x4b,
        Numpad5 => 0x4c,
        Numpad6 => 0x4d,
        NumpadAdd => 0x4e,
        Numpad1 => 0x4f,
        Numpad2 => 0x50,
        Numpad3 => 0x51,
        Numpad0 => 0x52,
        F11 => 0x57,
        F12 => 0x58,
        NumpadEnter => 0x9c,
        ControlRight => 0x9d,
        NumpadDivide => 0xb5,
        AltRight => 0xb8,
        Home => 0xc7,
        ArrowUp => 0xc8,
        PageUp => 0xc9,
        ArrowLeft => 0xcb,
        ArrowRight => 0xcd,
        End => 0xcf,
        ArrowDown => 0xd0,
        PageDown => 0xd1,
        Insert => 0xd2,
        Delete => 0xd3,
        Unknown => return None,
    })
}
