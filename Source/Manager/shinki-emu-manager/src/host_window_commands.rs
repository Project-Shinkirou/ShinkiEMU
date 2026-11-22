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

pub(super) fn set_game_fullscreen(
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
    fullscreen: bool,
) -> Result<(), String> {
    use slint::ComponentHandle;
    #[cfg(target_os = "android")]
    crate::platform::android::set_game_window(true, fullscreen)?;
    window.window().set_fullscreen(fullscreen);
    window.global::<Session>().set_fullscreen(fullscreen);
    Ok(())
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
        // Android owns the Activity icon and surface size. Valid hints are
        // accepted without mutating either; return the actual state below.
        #[cfg(target_os = "android")]
        WindowCommand::Icon | WindowCommand::Size { .. } => {}
        #[cfg(target_env = "ohos")]
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
            set_game_fullscreen(window, fullscreen)?;
        }
        #[cfg(target_env = "ohos")]
        WindowCommand::Size { .. } => return Err("SHINKI_EMU_WINDOW_COMMAND_UNSUPPORTED".into()),
        WindowCommand::Position { .. } | WindowCommand::Visibility(_) => {
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

#[cfg(all(test, target_os = "android"))]
mod android_tests {
    use super::*;
    use crate::{
        HostWake, modal_service::ModalBridge,
        window_service::{WindowBridge, WindowOutcome},
    };
    use shinki_emu_abi as abi;
    use shinki_emu_manager_ui_slint::SlintManagerAdapter;
    use slint::{
        ComponentHandle, PhysicalSize, PlatformError,
        platform::{
            Platform, WindowAdapter,
            software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
        },
    };
    use std::{rc::Rc, sync::{Arc, mpsc}, time::Duration};

    struct SoftwarePlatform(Rc<MinimalSoftwareWindow>);

    impl Platform for SoftwarePlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.0.clone())
        }
    }

    fn raw(kind: u32) -> abi::WindowRequest {
        let mut request: abi::WindowRequest = unsafe { std::mem::zeroed() };
        request.kind = kind;
        request.session_epoch = 1;
        request.context_epoch = 1;
        request.request_id = 1;
        request
    }

    fn call(bridge: &WindowBridge, request: &abi::WindowRequest) -> (abi::Status, WindowOutcome) {
        let sink = bridge.sink();
        let mut result: abi::WindowResult = unsafe { std::mem::zeroed() };
        let status = unsafe {
            sink.request.unwrap()(sink.context, request, &mut result, &mut abi::Error::default())
        };
        (status, WindowOutcome {
            valid_fields: result.valid_fields,
            width: result.width,
            height: result.height,
            fullscreen: result.fullscreen,
            minimized: result.minimized,
            ..WindowOutcome::default()
        })
    }

    #[test]
    fn decorations_and_size_are_advisory_without_weakening_window_service() {
        let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(SoftwarePlatform(software.clone()))).unwrap();
        let adapter = Rc::new(SlintManagerAdapter::new().unwrap());
        let pointer = super::super::pointer::HostPointer::install(&adapter);
        let window = adapter.window();
        let actual = PhysicalSize::new(900, 600);
        software.set_size(actual);

        let (sender, wakes) = mpsc::channel();
        let wake: HostWake = Arc::new(move || { let _ = sender.send(()); });
        let bridge = WindowBridge::new(1, ModalBridge::new(1, wake.clone()).unwrap(), wake);
        let dispatch = |kind, expected_error: Option<&str>| {
            let worker_bridge = bridge.clone();
            let worker = std::thread::spawn(move || {
                let mut request = raw(kind);
                if kind == abi::WINDOW_ICON {
                    request.width = 1;
                    request.height = 1;
                    request.pixels = abi::Bytes::new(&[255, 0, 0, 255]);
                } else {
                    request.width = 320;
                    request.height = 240;
                }
                call(&worker_bridge, &request)
            });
            wakes.recv_timeout(Duration::from_secs(5)).unwrap();
            let presentation = bridge.take().unwrap();
            let response = execute(window, &pointer, &presentation.command);
            assert_eq!(response.as_ref().err().map(String::as_str), expected_error);
            bridge.respond(&presentation, response).unwrap();
            worker.join().unwrap()
        };

        let (status, before) = dispatch(abi::WINDOW_QUERY_STATE, None);
        assert_eq!(status, abi::OK);
        for kind in [abi::WINDOW_ICON, abi::WINDOW_SIZE] {
            let (status, after) = dispatch(kind, None);
            assert_eq!(status, abi::OK);
            assert_eq!(window.window().size(), actual);
            assert_eq!((after.width, after.height), (actual.width, actual.height));
            assert_eq!(after.valid_fields, abi::WINDOW_RESULT_SIZE | abi::WINDOW_RESULT_MODE | abi::WINDOW_RESULT_MINIMIZED);
            assert_eq!(after.fullscreen, before.fullscreen);
            assert_eq!(after.minimized, before.minimized);
        }
        for kind in [abi::WINDOW_POSITION, abi::WINDOW_VISIBILITY] {
            let (status, outcome) = dispatch(kind, Some("SHINKI_EMU_WINDOW_COMMAND_UNSUPPORTED"));
            assert_eq!(status, abi::CORE_ERROR);
            assert_eq!(outcome.valid_fields, 0);
        }

        let mut invalid_icon = raw(abi::WINDOW_ICON);
        invalid_icon.width = 1;
        invalid_icon.height = 1;
        invalid_icon.pixels = abi::Bytes::new(&[255, 0, 0]);
        assert_eq!(call(&bridge, &invalid_icon).0, abi::INVALID_ARGUMENT);
        assert_eq!(call(&bridge, &raw(abi::WINDOW_SIZE)).0, abi::UNSUPPORTED);
        assert_eq!(call(&bridge, &raw(u32::MAX)).0, abi::UNSUPPORTED);
        assert!(wakes.try_recv().is_err());

        let worker_bridge = bridge.clone();
        let worker = std::thread::spawn(move || {
            let mut request = raw(abi::WINDOW_SIZE);
            request.width = 320;
            request.height = 240;
            call(&worker_bridge, &request).0
        });
        wakes.recv_timeout(Duration::from_secs(5)).unwrap();
        let presentation = bridge.take().unwrap();
        let sink = bridge.sink();
        unsafe { sink.invalidate.unwrap()(sink.context, 1, 2) };
        assert_eq!(worker.join().unwrap(), abi::CANCELLED);
        assert!(bridge.respond(&presentation, Ok(before)).is_err());
        assert_eq!(window.window().size(), actual);
        bridge.close();
    }
}
