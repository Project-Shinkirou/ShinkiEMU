//! Store sign-in in an embedded webview. The selected provider's
//! [`WebLogin`] rule says where sign-in starts and how a finished sign-in
//! looks; the host shows that page in the platform webview with a fresh,
//! non-persistent cookie store, reads the store cookies (HttpOnly included)
//! straight from the webview once the rule holds, closes the webview and
//! hands the cookies to the controller. Only the provider's stored login
//! (system secret store) outlives the webview.
//!
//! - Windows (WebView2), macOS and iOS (WKWebView): a child view of the
//!   Manager window placed over `StoreWebArea`'s page rectangle.
//! - Linux (WebKitGTK): a GTK window of its own, pumped from the tick.
//! - Android: `android.webkit.WebView` in a dialog above the activity.

use super::*;
use shinki_emu_manager_ui_slint::{ManagerWindow, Store};
use shinki_emu_store::{WebCookie, WebLogin};
use std::{cell::RefCell, path::Path, time::Duration};

/// Page area, closing and GTK events are handled every tick.
const TICK: Duration = Duration::from_millis(16);
/// On a completion page the cookie store is read at most this often.
const COOKIE_INTERVAL: Duration = Duration::from_millis(500);

struct Active {
    session: Session,
    rule: &'static WebLogin,
    last_read: Option<Instant>,
    _timer: slint::Timer,
}

enum Step {
    Pending,
    Closed,
    SignedIn(Vec<WebCookie>),
    Failed(String),
}

thread_local! {
    // Slint event-loop thread only.
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

/// Open the selected store's sign-in page.
pub(super) fn open<C: ManagerController>(w: &Wiring<C>) {
    if ACTIVE.with(|active| active.try_borrow().map_or(true, |a| a.is_some())) {
        return;
    }
    let Some(window) = w.window.upgrade() else {
        return;
    };
    let (rule, data_dir) = {
        let controller = w.controller.borrow();
        match controller.store_web_login() {
            Ok(rule) => (rule, controller.data_dir().join(".shinki").join("webview")),
            Err(error) => return w.show(Err(error)),
        }
    };
    window.global::<Store>().set_web_active(true);
    let session = match Session::open(&window, rule.start_url, &data_dir) {
        Ok(session) => session,
        Err(error) => {
            window.global::<Store>().set_web_active(false);
            return w.show(Err(format!("SHINKI_EMU_WEB_LOGIN_OPEN: {error}")));
        }
    };
    let timer = slint::Timer::default();
    let tick_wiring = w.clone();
    timer.start(slint::TimerMode::Repeated, TICK, move || tick(&tick_wiring));
    ACTIVE.with(|active| {
        *active.borrow_mut() = Some(Active {
            session,
            rule,
            last_read: None,
            _timer: timer,
        })
    });
}

/// Close the sign-in page without signing in.
pub(super) fn close<C: ManagerController>(w: &Wiring<C>) {
    finish(w, None);
}

fn tick<C: ManagerController>(w: &Wiring<C>) {
    let Some(window) = w.window.upgrade() else {
        return finish(w, None);
    };
    // Reading cookies runs a nested platform event loop that can fire this
    // timer again; the inner tick finds the slot borrowed and skips.
    let step = ACTIVE.with(|active| {
        let mut slot = active.try_borrow_mut().ok()?;
        let active = slot.as_mut()?;
        Some(if window.global::<App>().get_page() != "store" {
            Step::Closed
        } else {
            active.step(&window)
        })
    });
    match step {
        None | Some(Step::Pending) => {}
        Some(Step::Closed) => finish(w, None),
        Some(Step::SignedIn(cookies)) => finish(w, Some(Ok(cookies))),
        Some(Step::Failed(error)) => finish(w, Some(Err(error))),
    }
}

fn finish<C: ManagerController>(w: &Wiring<C>, result: Option<Result<Vec<WebCookie>, String>>) {
    let Some(active) = ACTIVE.with(|active| {
        active
            .try_borrow_mut()
            .ok()
            .and_then(|mut slot| slot.take())
    }) else {
        return;
    };
    if let Some(window) = w.window.upgrade() {
        window.global::<Store>().set_web_active(false);
    }
    // The webview and its timer go away after the current callback returns.
    slint::Timer::single_shot(Duration::ZERO, move || drop(active));
    match result {
        Some(Ok(cookies)) => w.run(|c| c.store_web_signed_in(cookies)),
        Some(Err(error)) => w.show(Err(format!("SHINKI_EMU_WEB_LOGIN: {error}"))),
        None => {}
    }
}

impl Active {
    fn step(&mut self, window: &ManagerWindow) -> Step {
        let store = window.global::<Store>();
        let area = Area {
            x: store.get_web_area_x(),
            y: store.get_web_area_y(),
            width: store.get_web_area_width(),
            height: store.get_web_area_height(),
            scale: window.window().scale_factor(),
        };
        let page = match self.session.page(area) {
            Ok(Some(page)) => page,
            Ok(None) => return Step::Closed,
            Err(error) => return Step::Failed(error),
        };
        if !self.rule.reached(&page)
            || self
                .last_read
                .is_some_and(|read| read.elapsed() < COOKIE_INTERVAL)
        {
            return Step::Pending;
        }
        self.last_read = Some(Instant::now());
        match self.session.cookies(self.rule) {
            Ok(cookies) => self
                .rule
                .finish(&page, cookies)
                .map_or(Step::Pending, Step::SignedIn),
            Err(error) => Step::Failed(error),
        }
    }
}

/// `StoreWebArea`'s page rectangle in logical window pixels.
#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos", target_os = "ios")),
    allow(dead_code)
)]
#[derive(Clone, Copy)]
struct Area {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios"))]
impl Area {
    /// Physical pixels on Windows and macOS (wry converts with the window's
    /// backing scale); points on iOS, where the frame is set directly.
    fn rect(self) -> wry::Rect {
        let scale = if cfg!(target_os = "ios") {
            1.0
        } else {
            self.scale
        };
        wry::Rect {
            position: wry::dpi::PhysicalPosition::new(
                (self.x * scale).round() as i32,
                (self.y * scale).round() as i32,
            )
            .into(),
            size: wry::dpi::PhysicalSize::new(
                (self.width * scale).round().max(0.0) as u32,
                (self.height * scale).round().max(0.0) as u32,
            )
            .into(),
        }
    }
}

/// Cookies of a wry webview's store, scoped by [`WebCookie::from_store`].
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    all(target_os = "linux", not(target_env = "ohos"))
))]
fn wry_cookies(webview: &wry::WebView) -> Result<Vec<WebCookie>, String> {
    webview
        .cookies()
        .map(|cookies| cookies.iter().filter_map(WebCookie::from_store).collect())
        .map_err(|error| error.to_string())
}

/// `window.open` from a sign-in page continues in the same webview: a popup
/// would escape the page area and, on some platforms, the private store.
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    all(target_os = "linux", not(target_env = "ohos"))
))]
fn sign_in_builder<'a>(
    context: &'a mut wry::WebContext,
    url: &str,
    popup: std::rc::Rc<RefCell<Option<String>>>,
) -> wry::WebViewBuilder<'a> {
    wry::WebViewBuilder::new_with_web_context(context)
        .with_url(url)
        .with_incognito(true)
        .with_new_window_req_handler(move |url, _| {
            *popup.borrow_mut() = Some(url);
            wry::NewWindowResponse::Deny
        })
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios"))]
struct Session {
    webview: wry::WebView,
    popup: std::rc::Rc<RefCell<Option<String>>>,
    placed: Option<(i32, i32, u32, u32)>,
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios"))]
impl Session {
    fn open(window: &ManagerWindow, url: &str, data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
        let handle = window.window().window_handle();
        // WebView2 keeps its browser profile here; incognito keeps cookies
        // out of it. WKWebView uses a non-persistent data store.
        let mut context = wry::WebContext::new(Some(data_dir.to_path_buf()));
        let popup = std::rc::Rc::default();
        let webview = sign_in_builder(&mut context, url, std::rc::Rc::clone(&popup))
            .with_bounds(wry::Rect::default())
            .build_as_child(&handle)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            webview,
            popup,
            placed: None,
        })
    }

    fn page(&mut self, area: Area) -> Result<Option<String>, String> {
        self.place(area)?;
        if let Some(url) = self.popup.borrow_mut().take() {
            self.webview.load_url(&url).map_err(|e| e.to_string())?;
        }
        self.webview.url().map(Some).map_err(|e| e.to_string())
    }

    fn place(&mut self, area: Area) -> Result<(), String> {
        let rect = area.rect();
        let position = rect.position.to_physical::<i32>(1.0);
        let size = rect.size.to_physical::<u32>(1.0);
        let placed = (position.x, position.y, size.width, size.height);
        if self.placed == Some(placed) {
            return Ok(());
        }
        self.placed = Some(placed);
        #[cfg(not(target_os = "ios"))]
        self.webview.set_bounds(rect).map_err(|e| e.to_string())?;
        // wry leaves child frames on iOS alone; set the frame in points.
        #[cfg(target_os = "ios")]
        {
            use wry::WebViewExtIOS;
            let frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(placed.0 as f64, placed.1 as f64),
                objc2_core_foundation::CGSize::new(placed.2 as f64, placed.3 as f64),
            );
            let view = self.webview.webview();
            // SAFETY: `setFrame:` takes a CGRect and is called on the main thread.
            let _: () = unsafe { objc2::msg_send![&*view, setFrame: frame] };
        }
        Ok(())
    }

    fn cookies(&mut self, _rule: &WebLogin) -> Result<Vec<WebCookie>, String> {
        wry_cookies(&self.webview)
    }
}

#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
struct Session {
    // Declared before `window` so the webview is destroyed first.
    webview: wry::WebView,
    window: gtk::Window,
    closed: std::rc::Rc<std::cell::Cell<bool>>,
    popup: std::rc::Rc<RefCell<Option<String>>>,
}

#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
impl Session {
    fn open(_window: &ManagerWindow, url: &str, data_dir: &Path) -> Result<Self, String> {
        use gtk::prelude::*;
        use wry::WebViewBuilderExtUnix;
        gtk::init().map_err(|error| error.to_string())?;
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(&shinki_emu_manager_ui_slint::tr!("登录"));
        window.set_default_size(960, 720);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&container);
        let closed = std::rc::Rc::new(std::cell::Cell::new(false));
        let on_close = std::rc::Rc::clone(&closed);
        window.connect_delete_event(move |_, _| {
            on_close.set(true);
            gtk::glib::Propagation::Stop
        });
        let mut context = wry::WebContext::new(Some(data_dir.to_path_buf()));
        let popup = std::rc::Rc::default();
        let webview = sign_in_builder(&mut context, url, std::rc::Rc::clone(&popup))
            .build_gtk(&container)
            .map_err(|error| error.to_string())?;
        window.show_all();
        Ok(Self {
            webview,
            window,
            closed,
            popup,
        })
    }

    fn page(&mut self, _area: Area) -> Result<Option<String>, String> {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        if self.closed.get() {
            return Ok(None);
        }
        if let Some(url) = self.popup.borrow_mut().take() {
            self.webview.load_url(&url).map_err(|e| e.to_string())?;
        }
        self.webview.url().map(Some).map_err(|e| e.to_string())
    }

    fn cookies(&mut self, _rule: &WebLogin) -> Result<Vec<WebCookie>, String> {
        wry_cookies(&self.webview)
    }
}

#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
impl Drop for Session {
    fn drop(&mut self) {
        use gtk::prelude::*;
        // SAFETY: the window is owned here and not used after destruction.
        unsafe { self.window.destroy() };
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}

/// `MainActivity`'s sign-in WebView; Android's cookie jar is wiped when the
/// WebView opens and closes, so captured cookies live only in the provider.
#[cfg(target_os = "android")]
struct Session;

#[cfg(target_os = "android")]
impl Session {
    fn open(_window: &ManagerWindow, url: &str, _data_dir: &Path) -> Result<Self, String> {
        platform::android::open_web_login(url)?;
        Ok(Self)
    }

    fn page(&mut self, _area: Area) -> Result<Option<String>, String> {
        platform::android::web_login_page()
    }

    /// `CookieManager` reports the cookies sent to a URL (HttpOnly included)
    /// as a request header, so each store origin is asked separately.
    fn cookies(&mut self, rule: &WebLogin) -> Result<Vec<WebCookie>, String> {
        let mut cookies = Vec::new();
        for origin in rule.cookie_urls {
            let Ok(url) = reqwest::Url::parse(origin) else {
                continue;
            };
            if let Some(header) = platform::android::web_login_cookies(origin)? {
                cookies.extend(WebCookie::from_request_header(&url, &header));
            }
        }
        Ok(cookies)
    }
}

#[cfg(target_os = "android")]
impl Drop for Session {
    fn drop(&mut self) {
        if let Err(error) = platform::android::close_web_login() {
            tracing::warn!(event = "shinki.emu.android.web_login_close_failed", error = %error);
        }
    }
}

/// OpenHarmony has no Manager webview host; `can_web` stays false there.
#[cfg(target_env = "ohos")]
struct Session;

#[cfg(target_env = "ohos")]
impl Session {
    fn open(_window: &ManagerWindow, _url: &str, _data_dir: &Path) -> Result<Self, String> {
        Err("SHINKI_EMU_WEB_LOGIN_UNSUPPORTED".into())
    }

    fn page(&mut self, _area: Area) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn cookies(&mut self, _rule: &WebLogin) -> Result<Vec<WebCookie>, String> {
        Ok(Vec::new())
    }
}
