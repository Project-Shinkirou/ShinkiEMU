//! Window-thread cursor ownership. Slint owns cursor shapes; this module owns
//! visibility and reapplies it after Slint handles each native pointer event.
//! Warping and input use the same letterbox and DPI geometry.
//! Android has no warpable cursor: visibility goes through the activity's
//! pointer icon and position requests are rejected.
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use shinki_emu_manager_core::model::FamilyPointerRequest;
use shinki_emu_manager_ui_slint::{Dialogs, ManagerWindow, Session, SlintManagerAdapter};
use slint::ComponentHandle;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
use slint::winit_030::WinitWindowAccessor;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
use winit::{dpi::PhysicalPosition, event::WindowEvent};

pub(super) struct HostPointer {
    visibility: PointerVisibility,
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    position: Option<PhysicalPosition<f64>>,
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    inside_window: bool,
    timer: slint::Timer,
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    after_event: slint::Timer,
}

struct PointerVisibility {
    visible: bool,
    autohide: Option<Duration>,
    idle_hidden: bool,
    last_activity: Instant,
    was_active: bool,
    controlling: bool,
}

impl PointerVisibility {
    fn new(now: Instant) -> Self {
        Self {
            visible: true,
            autohide: None,
            idle_hidden: false,
            last_activity: now,
            was_active: false,
            controlling: false,
        }
    }

    fn activity(&mut self, now: Instant) {
        self.last_activity = now;
        self.idle_hidden = false;
    }

    fn apply(&mut self, request: &FamilyPointerRequest, autohide: Option<Duration>, now: Instant) {
        if let Some(visible) = request.visible {
            self.visible = visible;
        }
        if let Some(duration) = autohide {
            self.autohide = (!duration.is_zero()).then_some(duration);
        }
        if request.visible.is_some()
            || autohide.is_some()
            || request.logical_x.is_some()
            || request.logical_y.is_some()
        {
            self.activity(now);
        }
    }

    fn synchronize(&mut self, active: bool, controlling: bool, now: Instant) -> bool {
        if !active && self.was_active {
            *self = Self::new(now);
        }
        if controlling && !self.controlling {
            // Returning from host UI/focus starts a fresh idle interval, but
            // never overrides an explicit game request to hide the cursor.
            self.activity(now);
        }
        self.was_active = active;
        self.controlling = controlling;
        !controlling || (self.visible && !self.idle_hidden)
    }

    fn delay(&self, now: Instant) -> Option<Duration> {
        if self.controlling && self.visible && !self.idle_hidden {
            self.autohide
                .map(|duration| duration.saturating_sub(now.duration_since(self.last_activity)))
        } else {
            None
        }
    }

    fn expire(&mut self, now: Instant) {
        if self.delay(now).is_some_and(|delay| delay.is_zero()) {
            self.idle_hidden = true;
        }
    }
}

impl HostPointer {
    pub(super) fn install(adapter: &Rc<SlintManagerAdapter>) -> Rc<RefCell<Self>> {
        let pointer = Rc::new(RefCell::new(Self {
            visibility: PointerVisibility::new(Instant::now()),
            #[cfg(not(any(target_os = "android", target_env = "ohos")))]
            position: None,
            #[cfg(not(any(target_os = "android", target_env = "ohos")))]
            inside_window: false,
            timer: slint::Timer::default(),
            #[cfg(not(any(target_os = "android", target_env = "ohos")))]
            after_event: slint::Timer::default(),
        }));
        let weak_pointer = Rc::downgrade(&pointer);
        let weak_window = adapter.window().as_weak();
        adapter
            .window()
            .global::<Session>()
            .on_pointer_state_changed(move || {
                if let (Some(pointer), Some(window)) =
                    (weak_pointer.upgrade(), weak_window.upgrade())
                {
                    Self::synchronize(&pointer, &window);
                }
            });
        pointer
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(super) fn apply(
        pointer: &Rc<RefCell<Self>>,
        window: &ManagerWindow,
        request: FamilyPointerRequest,
    ) -> Result<(), String> {
        window
            .window()
            .with_winit_window(|native| {
                let handle = native
                    .window_handle()
                    .map_err(|error| format!("SHINKI_EMU_HOST_WINDOW_UNAVAILABLE: {error}"))?;
                if !matches!(
                    handle.as_raw(),
                    RawWindowHandle::Win32(_)
                        | RawWindowHandle::Xlib(_)
                        | RawWindowHandle::Xcb(_)
                        | RawWindowHandle::Wayland(_)
                        | RawWindowHandle::AppKit(_)
                ) {
                    return Err("SHINKI_EMU_HOST_POINTER_UNAVAILABLE".to_owned());
                }
                Ok(())
            })
            .ok_or_else(|| "SHINKI_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())??;
        let autohide = request.autohide_ms.map(Duration::from_millis);
        if autohide.is_some_and(|duration| Instant::now().checked_add(duration).is_none()) {
            return Err("SHINKI_EMU_HOST_POINTER_DURATION".into());
        }
        let x = request.logical_x;
        let y = request.logical_y;
        if x.is_some() || y.is_some() {
            let current = pointer.borrow().position;
            let session = window.global::<Session>();
            let rect = [
                session.get_pointer_x() as f64,
                session.get_pointer_y() as f64,
                session.get_pointer_width() as f64,
                session.get_pointer_height() as f64,
            ];
            let stage = [
                session.get_native_width() as f64,
                session.get_native_height() as f64,
            ];
            let position = window
                .window()
                .with_winit_window(|native| {
                    let position = map_position(x, y, current, rect, stage, native.scale_factor())?;
                    native.set_cursor_position(position).map_err(|error| {
                        format!("SHINKI_EMU_HOST_POINTER_POSITION_UNAVAILABLE: {error}")
                    })?;
                    Ok::<_, String>(position)
                })
                .ok_or_else(|| "SHINKI_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())??;
            pointer.borrow_mut().position = Some(position);
        }
        pointer
            .borrow_mut()
            .visibility
            .apply(&request, autohide, Instant::now());
        Self::synchronize(pointer, window);
        Ok(())
    }

    #[cfg(any(target_os = "android", target_env = "ohos"))]
    pub(super) fn apply(
        pointer: &Rc<RefCell<Self>>,
        window: &ManagerWindow,
        request: FamilyPointerRequest,
    ) -> Result<(), String> {
        let autohide = request.autohide_ms.map(Duration::from_millis);
        if autohide.is_some_and(|duration| Instant::now().checked_add(duration).is_none()) {
            return Err("SHINKI_EMU_HOST_POINTER_DURATION".into());
        }
        if request.logical_x.is_some() || request.logical_y.is_some() {
            return Err("SHINKI_EMU_HOST_POINTER_POSITION_UNAVAILABLE".into());
        }
        pointer
            .borrow_mut()
            .visibility
            .apply(&request, autohide, Instant::now());
        Self::synchronize(pointer, window);
        Ok(())
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(super) fn observe(
        pointer: &Rc<RefCell<Self>>,
        window: &ManagerWindow,
        event: &WindowEvent,
    ) {
        {
            let mut state = pointer.borrow_mut();
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    state.position = Some(*position);
                    state.inside_window = true;
                    state.visibility.activity(Instant::now());
                }
                WindowEvent::MouseInput { .. } => {
                    state.visibility.activity(Instant::now());
                }
                WindowEvent::CursorEntered { .. } => state.inside_window = true,
                WindowEvent::CursorLeft { .. } => state.inside_window = false,
                WindowEvent::Focused(_) => {}
                _ => return,
            }
        }
        Self::synchronize(pointer, window);
        // The native event observer runs before Slint. A cursor-shape change
        // in Slint also sets visibility, so settle visibility after dispatch.
        let weak_pointer = Rc::downgrade(pointer);
        let weak_window = window.as_weak();
        pointer.borrow().after_event.start(
            slint::TimerMode::SingleShot,
            Duration::ZERO,
            move || {
                if let (Some(pointer), Some(window)) =
                    (weak_pointer.upgrade(), weak_window.upgrade())
                {
                    Self::synchronize(&pointer, &window);
                }
            },
        );
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    fn synchronize(pointer: &Rc<RefCell<Self>>, window: &ManagerWindow) {
        let mut state = pointer.borrow_mut();
        state.timer.stop();
        let session = window.global::<Session>();
        let dialogs = window.global::<Dialogs>();
        let active = session.get_active();
        let controlling = active
            && !dialogs.get_fatal_active()
            && !dialogs.get_modal_active()
            && !session.get_menu_open()
            && state.inside_window
            && window
                .window()
                .with_winit_window(|native| {
                    native.has_focus()
                        && state.position.is_some_and(|position| {
                            let x = position.x / native.scale_factor();
                            let y = position.y / native.scale_factor();
                            let left = session.get_pointer_x() as f64;
                            let top = session.get_pointer_y() as f64;
                            x >= left
                                && y >= top
                                && x < left + session.get_pointer_width() as f64
                                && y < top + session.get_pointer_height() as f64
                        })
                })
                .unwrap_or(false);
        let now = Instant::now();
        let displayed = state.visibility.synchronize(active, controlling, now);
        // Do not cache our last write: Slint changes the same native cursor
        // when its hovered control changes (including none -> default).
        window
            .window()
            .with_winit_window(|native| native.set_cursor_visible(displayed));
        Self::schedule_expiry(pointer, &state, window, now);
    }

    /// Mobile platforms report no hover position and touch shares the native
    /// surface, so the stage controls the pointer whenever no host UI covers it.
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    pub(super) fn synchronize(pointer: &Rc<RefCell<Self>>, window: &ManagerWindow) {
        let mut state = pointer.borrow_mut();
        state.timer.stop();
        let session = window.global::<Session>();
        let dialogs = window.global::<Dialogs>();
        let active = session.get_active();
        let controlling = active
            && !dialogs.get_fatal_active()
            && !dialogs.get_modal_active()
            && !session.get_menu_open();
        let now = Instant::now();
        let displayed = state.visibility.synchronize(active, controlling, now);
        #[cfg(target_os = "android")]
        let hidden = crate::platform::android::set_pointer_hidden(!displayed);
        #[cfg(target_env = "ohos")]
        let hidden = crate::platform::ohos::set_pointer_hidden(!displayed);
        if let Err(error) = hidden {
            tracing::warn!(
                event = "shinki.emu.host.pointer_visibility_failed",
                diagnostic_code = "SHINKI_EMU_HOST_POINTER_UNAVAILABLE",
                error = %error
            );
        }
        Self::schedule_expiry(pointer, &state, window, now);
    }

    fn schedule_expiry(
        pointer: &Rc<RefCell<Self>>,
        state: &Self,
        window: &ManagerWindow,
        now: Instant,
    ) {
        if let Some(delay) = state.visibility.delay(now) {
            let weak_pointer = Rc::downgrade(pointer);
            let weak_window = window.as_weak();
            state
                .timer
                .start(slint::TimerMode::SingleShot, delay, move || {
                    if let (Some(pointer), Some(window)) =
                        (weak_pointer.upgrade(), weak_window.upgrade())
                    {
                        pointer.borrow_mut().visibility.expire(Instant::now());
                        Self::synchronize(&pointer, &window);
                    }
                });
        }
    }
}

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
fn map_position(
    x: Option<i32>,
    y: Option<i32>,
    current: Option<PhysicalPosition<f64>>,
    rect: [f64; 4],
    stage: [f64; 2],
    scale: f64,
) -> Result<PhysicalPosition<f64>, String> {
    if !rect
        .into_iter()
        .chain(stage)
        .chain([scale])
        .all(f64::is_finite)
        || rect[2] <= 0.0
        || rect[3] <= 0.0
        || stage[0] <= 0.0
        || stage[1] <= 0.0
        || scale <= 0.0
    {
        return Err("SHINKI_EMU_HOST_POINTER_VIEWPORT_UNAVAILABLE".into());
    }
    let missing = || "SHINKI_EMU_HOST_POINTER_POSITION_UNAVAILABLE".to_owned();
    let x = match x {
        Some(x) => (rect[0] + x as f64 / stage[0] * rect[2]) * scale,
        None => current.ok_or_else(missing)?.x,
    };
    let y = match y {
        Some(y) => (rect[1] + y as f64 / stage[1] * rect[3]) * scale,
        None => current.ok_or_else(missing)?.y,
    };
    Ok(PhysicalPosition::new(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configure(
        state: &mut PointerVisibility,
        visible: Option<bool>,
        ms: Option<u64>,
        now: Instant,
    ) {
        state.apply(
            &FamilyPointerRequest {
                visible,
                autohide_ms: ms,
                ..Default::default()
            },
            ms.map(Duration::from_millis),
            now,
        );
    }

    #[test]
    fn idle_hiding_is_reversed_by_activity_but_explicit_hiding_is_not() {
        let now = Instant::now();
        let mut state = PointerVisibility::new(now);
        configure(&mut state, None, Some(1000), now);
        assert!(state.synchronize(true, true, now));
        let expired = now + Duration::from_millis(1000);
        state.expire(expired);
        assert!(!state.synchronize(true, true, expired));
        state.activity(expired);
        assert!(state.synchronize(true, true, expired));
        assert_eq!(state.delay(expired), Some(Duration::from_millis(1000)));
        configure(&mut state, Some(false), None, expired);
        state.activity(expired);
        assert!(!state.synchronize(true, true, expired));
        assert_eq!(state.delay(expired), None);
        configure(&mut state, Some(true), Some(0), expired);
        assert!(state.synchronize(true, true, expired));
        assert_eq!(state.delay(expired), None);
    }

    #[test]
    fn host_modal_overlay_toolbar_and_focus_suspend_idle_hiding() {
        let now = Instant::now();
        for explicit in [false, true] {
            let mut state = PointerVisibility::new(now);
            configure(&mut state, Some(!explicit), Some(1000), now);
            state.synchronize(true, true, now);
            let expired = now + Duration::from_secs(1);
            state.expire(expired);
            assert!(!state.synchronize(true, true, expired));
            assert!(state.synchronize(true, false, expired));
            assert_eq!(state.delay(expired), None);
            let returned = expired + Duration::from_secs(60);
            state.expire(returned);
            assert_eq!(state.synchronize(true, true, returned), !explicit);
            assert_eq!(
                state.delay(returned),
                (!explicit).then_some(Duration::from_millis(1000))
            );
        }
    }

    #[test]
    fn exiting_and_reopening_discards_both_hidden_modes() {
        let now = Instant::now();
        for explicit in [false, true] {
            let mut state = PointerVisibility::new(now);
            configure(&mut state, Some(!explicit), Some(1), now);
            state.synchronize(true, true, now);
            let expired = now + Duration::from_millis(1);
            state.expire(expired);
            assert!(!state.synchronize(true, true, expired));
            assert!(state.synchronize(false, false, expired));
            assert!(state.synchronize(true, true, expired));
            assert_eq!(state.delay(expired), None);
        }
    }

    #[test]
    fn repeated_visibility_sync_keeps_the_current_request_and_deadline() {
        let now = Instant::now();
        let mut state = PointerVisibility::new(now);
        configure(&mut state, None, Some(1000), now);
        state.synchronize(true, true, now);
        let moved = now + Duration::from_millis(500);
        assert!(state.synchronize(true, true, moved));
        assert_eq!(state.delay(moved), Some(Duration::from_millis(500)));
        configure(&mut state, Some(false), None, moved);
        assert!(!state.synchronize(true, true, moved));
        assert!(!state.synchronize(true, true, moved));
        assert!(state.synchronize(true, false, moved));
        assert!(!state.synchronize(true, true, moved));
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    #[test]
    fn stage_warp_uses_content_viewport_dpi_and_preserves_other_axis() {
        let rect = [100.0, 80.0, 640.0, 360.0];
        assert_eq!(
            map_position(Some(640), Some(360), None, rect, [1280.0, 720.0], 1.5).unwrap(),
            PhysicalPosition::new(630.0, 390.0)
        );
        assert_eq!(
            map_position(
                Some(0),
                None,
                Some(PhysicalPosition::new(50.0, 123.0)),
                rect,
                [1280.0, 720.0],
                1.5
            )
            .unwrap(),
            PhysicalPosition::new(150.0, 123.0)
        );
        assert!(map_position(None, Some(0), None, rect, [1280.0, 720.0], 1.0).is_err());
        assert!(map_position(Some(0), Some(0), None, rect, [0.0, 720.0], 1.0).is_err());
    }
}
