use crate::runtime::{KeyCode, KeyModifiers, KeyState, PointerButton, RuntimeEvent};
use crate::{StageCanvas, native::ServiceContext};
use shinki_emu_abi as abi;

/// Own ordered input values before handing them to the core worker.
///
/// # Safety
/// Each byte span in `source` must remain readable for its declared length
/// until this function returns.
pub unsafe fn translate(
    services: &ServiceContext,
    canvas: &StageCanvas,
    source: &[abi::InputEvent],
) -> Result<Vec<RuntimeEvent>, abi::Status> {
    let mut events = Vec::with_capacity(source.len());
    for event in source {
        if event.pressed > 1 || event.repeat > 1 {
            return Err(abi::INVALID_ARGUMENT);
        }
        let state = if event.pressed == 0 {
            KeyState::Released
        } else if event.repeat != 0 {
            KeyState::Repeated
        } else {
            KeyState::Pressed
        };
        match event.kind {
            abi::INPUT_KEY => {
                let physical = unsafe { event.physical_key.text(128) }?;
                if physical.is_empty() {
                    if state != KeyState::Pressed {
                        return Err(abi::INVALID_ARGUMENT);
                    }
                    let text = unsafe { event.logical_key.text(crate::runtime::MAX_TEXT_BYTES) }?;
                    events.push(RuntimeEvent::TextInput { text: text.into() });
                    continue;
                }
                let code = key(physical);
                if code == KeyCode::F9 && state == KeyState::Pressed {
                    services.invalidate();
                }
                events.push(RuntimeEvent::Key {
                    code,
                    state,
                    modifiers: KeyModifiers {
                        shift: false,
                        control: false,
                        alt: false,
                        super_key: false,
                    },
                });
            }
            abi::INPUT_POINTER_MOVE | abi::INPUT_POINTER_BUTTON => {
                if event.displayed.session_epoch != services.session
                    || event.displayed.output_epoch != 1
                    || event.displayed.generation == 0
                {
                    return Err(abi::STALE_CONTEXT);
                }
                if !event.x.is_finite()
                    || !event.y.is_finite()
                    || event.x.abs() > f64::from(f32::MAX)
                    || event.y.abs() > f64::from(f32::MAX)
                {
                    return Err(abi::INVALID_ARGUMENT);
                }
                let button = if event.kind == abi::INPUT_POINTER_BUTTON {
                    Some(match event.button {
                        abi::BUTTON_PRIMARY => PointerButton::Primary,
                        abi::BUTTON_SECONDARY => PointerButton::Secondary,
                        abi::BUTTON_MIDDLE => PointerButton::Middle,
                        _ => return Err(abi::INVALID_ARGUMENT),
                    })
                } else {
                    None
                };
                let raster = [event.x as f32, event.y as f32];
                if !canvas.contains_raster_point(raster) {
                    if state == KeyState::Released
                        && let Some(button) = button
                    {
                        events.push(RuntimeEvent::PointerButton { button, state });
                    }
                    continue;
                }
                let [x, y] = canvas
                    .raster_to_logical_point(raster)
                    .map_err(|_| abi::INVALID_ARGUMENT)?;
                events.push(RuntimeEvent::PointerMove { x, y });
                if let Some(button) = button {
                    events.push(RuntimeEvent::PointerButton { button, state });
                }
            }
            abi::INPUT_WHEEL => {
                if !event.x.is_finite()
                    || !event.y.is_finite()
                    || event.x.abs() > f64::from(f32::MAX)
                    || event.y.abs() > f64::from(f32::MAX)
                {
                    return Err(abi::INVALID_ARGUMENT);
                }
                events.push(RuntimeEvent::Wheel {
                    delta_x: event.x as f32,
                    delta_y: event.y as f32,
                });
            }
            abi::INPUT_WINDOW => {
                if event.window.focused > 1 || event.window.suspended > 1 {
                    return Err(abi::INVALID_ARGUMENT);
                }
                events.push(RuntimeEvent::WindowFocused {
                    focused: event.window.focused != 0,
                });
                events.push(RuntimeEvent::WindowSuspended {
                    suspended: event.window.suspended != 0,
                });
            }
            _ => return Err(abi::UNSUPPORTED),
        }
    }
    Ok(events)
}
fn key(value: &str) -> KeyCode {
    match value {
        "Equal" => return KeyCode::Equals,
        "Backquote" => return KeyCode::Grave,
        "Quote" => return KeyCode::Apostrophe,
        _ => {}
    }
    let value = value.strip_prefix("Key").unwrap_or(value);
    macro_rules! keys { ($($name:ident),* $(,)?) => { match value { $(stringify!($name) => KeyCode::$name,)* _ => KeyCode::Unknown } }; }
    keys!(
        A,
        B,
        C,
        D,
        E,
        F,
        G,
        H,
        I,
        J,
        K,
        L,
        M,
        N,
        O,
        P,
        Q,
        R,
        S,
        T,
        U,
        V,
        W,
        X,
        Y,
        Z,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Escape,
        Enter,
        Tab,
        Backspace,
        Space,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
        ArrowLeft,
        ArrowRight,
        ArrowUp,
        ArrowDown,
        ControlLeft,
        ControlRight,
        ShiftLeft,
        ShiftRight,
        AltLeft,
        AltRight,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadEnter,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        Minus,
        Comma,
        Period,
        Slash,
        Semicolon,
        BracketLeft,
        BracketRight,
        Backslash
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use roukaku_media_core::Extent2D;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    fn key_event(physical: abi::Bytes, logical: abi::Bytes) -> abi::InputEvent {
        abi::InputEvent {
            kind: abi::INPUT_KEY,
            pressed: 1,
            repeat: 0,
            button: 0,
            physical_key: physical,
            logical_key: logical,
            x: 0.0,
            y: 0.0,
            displayed: abi::FrameIdentity::default(),
            window: abi::WindowState {
                drawable_width: 1,
                drawable_height: 1,
                scale_factor: 1.0,
                focused: 1,
                suspended: 0,
            },
        }
    }

    fn translate_keys(source: &[abi::InputEvent]) -> (Result<Vec<RuntimeEvent>, abi::Status>, u64) {
        let services = ServiceContext {
            session: 1,
            epoch: AtomicU64::new(1),
            cancelled: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            text: None,
            window: None,
            host_cancel: None,
        };
        let extent = Extent2D::new(1, 1);
        let canvas = StageCanvas::new(extent, extent).unwrap();
        let events = unsafe { translate(&services, &canvas, source) };
        (events, services.epoch.load(Ordering::Relaxed))
    }

    #[test]
    fn native_input_owns_utf8_commit_and_preserves_physical_keys() {
        let mut text = "日本語 café 🐟".to_owned();
        let logical = abi::Bytes::new(text.as_bytes());
        let (events, epoch) = translate_keys(&[
            key_event(abi::Bytes::from_static(b"KeyA"), logical),
            key_event(abi::Bytes::EMPTY, logical),
            key_event(abi::Bytes::from_static(b"F9"), abi::Bytes::EMPTY),
        ]);
        text.clear();
        assert!(matches!(
            events.unwrap().as_slice(),
            [
                RuntimeEvent::Key {
                    code: KeyCode::A,
                    state: KeyState::Pressed,
                    ..
                },
                RuntimeEvent::TextInput { text },
                RuntimeEvent::Key {
                    code: KeyCode::F9,
                    state: KeyState::Pressed,
                    ..
                },
            ] if text == "日本語 café 🐟"
        ));
        assert_eq!(epoch, 2);
    }

    #[test]
    fn native_input_rejects_malformed_text_commits() {
        let text = abi::Bytes::from_static(b"test");
        let commit = key_event(abi::Bytes::EMPTY, text);
        let too_long = vec![b'x'; crate::runtime::MAX_TEXT_BYTES + 1];
        for event in [
            abi::InputEvent {
                pressed: 0,
                ..commit
            },
            abi::InputEvent {
                repeat: 1,
                ..commit
            },
            key_event(abi::Bytes::EMPTY, abi::Bytes::from_static(b"\xff")),
            key_event(abi::Bytes::EMPTY, abi::Bytes::new(&too_long)),
        ] {
            assert!(matches!(translate_keys(&[event]).0, Err(abi::INVALID_ARGUMENT)));
        }
    }
}
