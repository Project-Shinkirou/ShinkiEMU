use shinki_emu_manager_core::model::FamilyWindowIcon;
use shinki_emu_manager_ui_slint::Session;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
use slint::winit_030::WinitWindowAccessor;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Let the window manager fit the initial window when the preferred UI size
/// exceeds the current display, including its native decorations.
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
pub(super) fn fit_initial_window(window: &slint::Window) {
    let oversized = window.with_winit_window(|window| {
        window.current_monitor().is_some_and(|monitor| {
            let available = monitor.size();
            let requested = window.outer_size();
            requested.width > available.width || requested.height > available.height
        })
    });
    if oversized == Some(true) {
        window.set_maximized(true);
    }
}

/// None restores the Manager application icon on desktop platforms.
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
pub(super) fn set_icon(
    window: &slint::Window,
    icon: Option<FamilyWindowIcon>,
) -> Result<(), String> {
    let custom = icon.is_some();
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let icon = icon.or_else(|| {
        Some(FamilyWindowIcon {
            width: 64,
            height: 64,
            rgba: include_bytes!(concat!(env!("OUT_DIR"), "/window-icon.rgba")).to_vec(),
        })
    });
    let icon = icon
        .map(|icon| {
            icon.validate().map_err(|error| error.to_string())?;
            winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height)
                .map_err(|_| "SHINKI_EMU_HOST_WINDOW_ICON_INVALID".to_owned())
        })
        .transpose()?;
    window
        .with_winit_window(|native| {
            if icon.is_some() {
                let handle = native
                    .window_handle()
                    .map_err(|_| "SHINKI_EMU_HOST_WINDOW_UNAVAILABLE")?;
                if !matches!(
                    handle.as_raw(),
                    RawWindowHandle::Win32(_) | RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
                ) {
                    return if custom {
                        Err("SHINKI_EMU_HOST_WINDOW_ICON_UNAVAILABLE".to_owned())
                    } else {
                        Ok(())
                    };
                }
            }
            native.set_window_icon(icon);
            Ok(())
        })
        .ok_or_else(|| "SHINKI_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())?
}

/// The activity has no per-window icon; None is the default state.
#[cfg(any(target_os = "android", target_env = "ohos"))]
pub(super) fn set_icon(
    _window: &slint::Window,
    icon: Option<FamilyWindowIcon>,
) -> Result<(), String> {
    match icon {
        None => Ok(()),
        Some(icon) => {
            icon.validate().map_err(|error| error.to_string())?;
            Err("SHINKI_EMU_HOST_WINDOW_ICON_UNAVAILABLE".to_owned())
        }
    }
}

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
pub(super) fn execute(
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
    pointer: &std::rc::Rc<std::cell::RefCell<super::pointer::HostPointer>>,
    command: &crate::window_service::WindowCommand,
) -> Result<crate::window_service::WindowOutcome, String> {
    use crate::window_service::{WindowCommand, WindowOutcome};
    use shinki_emu_abi as abi;
    use slint::ComponentHandle;
    match command {
        WindowCommand::Icon(icon) => set_icon(window.window(), Some(icon.clone()))?,
        WindowCommand::Pointer(request) => {
            super::pointer::HostPointer::apply(pointer, window, request.clone())?
        }
        _ => {}
    }
    window
        .window()
        .with_winit_window(|native| {
            match command {
                WindowCommand::Title(title) => {
                    native.set_title(title);
                    window.global::<Session>().set_title(title.as_str().into());
                }
                WindowCommand::Fullscreen {
                    mode,
                    width,
                    height,
                } => {
                    let fullscreen = match *mode {
                        abi::FULLSCREEN_WINDOWED => None,
                        abi::FULLSCREEN_BORDERLESS => Some(winit::window::Fullscreen::Borderless(
                            native.current_monitor(),
                        )),
                        abi::FULLSCREEN_EXCLUSIVE => {
                            let monitor = native
                                .current_monitor()
                                .ok_or("SHINKI_EMU_WINDOW_MONITOR")?;
                            let mode = monitor
                                .video_modes()
                                .filter(|mode| {
                                    mode.size() == winit::dpi::PhysicalSize::new(*width, *height)
                                })
                                .max_by_key(|mode| {
                                    (mode.refresh_rate_millihertz(), mode.bit_depth())
                                })
                                .ok_or("SHINKI_EMU_WINDOW_DISPLAY_MODE")?;
                            Some(winit::window::Fullscreen::Exclusive(mode))
                        }
                        _ => return Err("SHINKI_EMU_WINDOW_FULLSCREEN".into()),
                    };
                    window
                        .global::<Session>()
                        .set_fullscreen(fullscreen.is_some());
                    native.set_fullscreen(fullscreen);
                }
                WindowCommand::Size { width, height } => {
                    let _ =
                        native.request_inner_size(winit::dpi::PhysicalSize::new(*width, *height));
                }
                WindowCommand::Position { x, y } => {
                    native.set_outer_position(winit::dpi::PhysicalPosition::new(*x, *y))
                }
                WindowCommand::Visibility(value) => native.set_visible(*value),
                WindowCommand::Minimized(value) => native.set_minimized(*value),
                WindowCommand::QueryState | WindowCommand::Icon(_) | WindowCommand::Pointer(_) => {}
            }
            let size = native.inner_size();
            let mut result = WindowOutcome {
                valid_fields: abi::WINDOW_RESULT_SIZE | abi::WINDOW_RESULT_MODE,
                width: size.width,
                height: size.height,
                fullscreen: match native.fullscreen() {
                    None => abi::FULLSCREEN_WINDOWED,
                    Some(winit::window::Fullscreen::Borderless(_)) => abi::FULLSCREEN_BORDERLESS,
                    Some(winit::window::Fullscreen::Exclusive(_)) => abi::FULLSCREEN_EXCLUSIVE,
                },
                ..WindowOutcome::default()
            };
            if let Ok(position) = native.outer_position() {
                result.valid_fields |= abi::WINDOW_RESULT_POSITION;
                result.x = position.x;
                result.y = position.y;
            }
            if let Some(visible) = native.is_visible() {
                result.valid_fields |= abi::WINDOW_RESULT_VISIBILITY;
                result.visible = u32::from(visible);
            }
            if let Some(minimized) = native.is_minimized() {
                result.valid_fields |= abi::WINDOW_RESULT_MINIMIZED;
                result.minimized = u32::from(minimized);
            }
            Ok(result)
        })
        .ok_or_else(|| "SHINKI_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())?
}

/// The activity surface is always full-window: only the fullscreen flag and
/// minimizing are under host control.
#[cfg(any(target_os = "android", target_env = "ohos"))]
pub(super) fn execute(
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
    pointer: &std::rc::Rc<std::cell::RefCell<super::pointer::HostPointer>>,
    command: &crate::window_service::WindowCommand,
) -> Result<crate::window_service::WindowOutcome, String> {
    use crate::window_service::{WindowCommand, WindowOutcome};
    use shinki_emu_abi as abi;
    use slint::ComponentHandle;
    let native = window.window();
    match command {
        WindowCommand::Icon(icon) => set_icon(native, Some(icon.clone()))?,
        WindowCommand::Pointer(request) => {
            super::pointer::HostPointer::apply(pointer, window, request.clone())?
        }
        WindowCommand::Title(title) => {
            window.global::<Session>().set_title(title.as_str().into());
        }
        WindowCommand::Fullscreen { mode, .. } => {
            let fullscreen = match *mode {
                abi::FULLSCREEN_WINDOWED => false,
                abi::FULLSCREEN_BORDERLESS => true,
                abi::FULLSCREEN_EXCLUSIVE => return Err("SHINKI_EMU_WINDOW_DISPLAY_MODE".into()),
                _ => return Err("SHINKI_EMU_WINDOW_FULLSCREEN".into()),
            };
            window.global::<Session>().set_fullscreen(fullscreen);
            native.set_fullscreen(fullscreen);
        }
        WindowCommand::Size { .. }
        | WindowCommand::Position { .. }
        | WindowCommand::Visibility(_) => {
            return Err("SHINKI_EMU_WINDOW_COMMAND_UNSUPPORTED".into());
        }
        WindowCommand::Minimized(value) => native.set_minimized(*value),
        WindowCommand::QueryState => {}
    }
    let size = native.size();
    Ok(WindowOutcome {
        valid_fields: abi::WINDOW_RESULT_SIZE
            | abi::WINDOW_RESULT_MODE
            | abi::WINDOW_RESULT_MINIMIZED,
        width: size.width,
        height: size.height,
        fullscreen: if native.is_fullscreen() {
            abi::FULLSCREEN_BORDERLESS
        } else {
            abi::FULLSCREEN_WINDOWED
        },
        minimized: u32::from(native.is_minimized()),
        ..WindowOutcome::default()
    })
}
