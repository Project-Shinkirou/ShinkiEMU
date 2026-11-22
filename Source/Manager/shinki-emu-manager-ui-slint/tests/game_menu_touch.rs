//! Touch reveal of the in-game menu button, driven through the real
//! `ManagerWindow` and Slint's touch pipeline on a 780x360 landscape phone.
//!
//! A touch on the game is delivered to the game unchanged and reveals the
//! corner button once it lifts; the button stays hidden while a game touch
//! is down, and only a tap on the visible button opens the menu.
//! The game fills the whole view, so the strip that used to start the
//! right-edge swipe lies inside the game. Game coordinates are twice the
//! view's logical pixels.
//!
//! Set `SHINKI_UI_SCREENSHOT_DIR` to keep `in-game/touch-<state>-<language>-780x360.png`
//! in zh-CN, en and ja; without it the checks run in ja only. The game frame
//! is a synthetic gradient.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use i_slint_core::{input::TouchPhase, items::AccessibleRole};
use shinki_emu_manager_ui_slint::{
    App, Dialogs, ManagerViewModel, MaterialWindowAdapter, Session, SlintManagerAdapter, i18n, tr,
};
use slint::{
    ComponentHandle, LogicalPosition, Rgba8Pixel, SharedPixelBuffer, SharedString,
    platform::{
        InternalEvent, Key, Platform, PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
    private_unstable_api::re_exports::{
        AccessibilityAction, AccessibleStringProperty, ItemRc, WindowInner,
    },
};

const W: u32 = 780;
const H: u32 = 360;
const GAME_SCALE: f32 = 2.0;
/// Centre of the 48px touch button, 16px in from the top-right corner.
const BUTTON: (f32, f32) = (W as f32 - 16.0 - 24.0, 16.0 + 24.0);

/// Software platform whose clock the test can move forward, so the 3 s idle
/// timer and the fade run without waiting.
struct SoftwarePlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
    skipped: Rc<Cell<Duration>>,
}

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.start.elapsed() + self.skipped.get()
    }
}

fn render(software: &MinimalSoftwareWindow) -> Vec<u8> {
    software.set_size(slint::PhysicalSize::new(W, H));
    software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    software.dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(W as f32, H as f32),
    });
    let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; (W * H) as usize];
    for frame in 0..60 {
        slint::platform::update_timers_and_animations();
        software.request_redraw();
        software.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, W as usize);
        });
        if frame >= 2 && !software.window().has_active_animations() {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    pixels.iter().flat_map(|p| [p.r, p.g, p.b, 255]).collect()
}

/// Synthetic game frame: a diagonal gradient with a light grid.
fn game_frame() -> slint::Image {
    let (w, h) = (390, 180);
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    for (i, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let t = (x + y) as f32 / (w + h) as f32;
        let grid = x % 30 == 0 || y % 30 == 0;
        *pixel = if grid {
            Rgba8Pixel { r: 200, g: 200, b: 210, a: 255 }
        } else {
            Rgba8Pixel { r: (40.0 + 150.0 * t) as u8, g: 70, b: (170.0 - 100.0 * t) as u8, a: 255 }
        };
    }
    slint::Image::from_rgba8(buffer)
}

fn model(reduce_motion: bool) -> ManagerViewModel {
    let mut m = ManagerViewModel::default();
    m.app.version = "0.1.0-beta.9".into();
    m.app.appearance.theme_mode = "dark".into();
    m.app.appearance.reduce_motion = reduce_motion;
    m.library.prepare_state = "ready".into();
    m.session.phase = "running".into();
    m.session.launch_title = "Harbor Lights".into();
    m
}

#[derive(Debug, Clone, PartialEq)]
struct Input(String, bool, f32);

fn pointer(x: f32, y: f32) -> Vec<Input> {
    vec![
        Input("pointer.x".into(), true, x * GAME_SCALE),
        Input("pointer.y".into(), true, y * GAME_SCALE),
    ]
}

fn down(x: f32, y: f32) -> Vec<Input> {
    let mut events = pointer(x, y);
    events.push(Input("pointer.primary".into(), true, 1.0));
    events
}

fn up(x: f32, y: f32) -> Vec<Input> {
    let mut events = pointer(x, y);
    events.push(Input("pointer.primary".into(), false, 1.0));
    events
}

fn assert_inputs(actual: Vec<Input>, expected: Vec<Vec<Input>>, what: &str) {
    let expected: Vec<Input> = expected.into_iter().flatten().collect();
    let same = actual.len() == expected.len()
        && actual
            .iter()
            .zip(&expected)
            .all(|(a, e)| a.0 == e.0 && a.1 == e.1 && (a.2 - e.2).abs() < 0.05);
    assert!(same, "{what}: game received {actual:?}, expected {expected:?}");
}

struct Fixture {
    adapter: SlintManagerAdapter,
    software: Rc<MinimalSoftwareWindow>,
    skipped: Rc<Cell<Duration>>,
    inputs: Rc<RefCell<Vec<Input>>>,
    output: Option<PathBuf>,
    language: Cell<&'static str>,
}

impl Fixture {
    fn save(&self, state: &str) {
        let rgba = render(&self.software);
        if let Some(dir) = &self.output {
            let dir = dir.join("in-game");
            std::fs::create_dir_all(&dir).expect("screenshot dir");
            image::RgbaImage::from_raw(W, H, rgba)
                .expect("frame size")
                .save(dir.join(format!("touch-{state}-{}-{W}x{H}.png", self.language.get())))
                .expect("write png");
        }
    }

    /// One frame at the current time, as the app draws after each event; this
    /// is what starts the fade animation.
    fn draw(&self) {
        slint::platform::update_timers_and_animations();
        self.software.request_redraw();
        self.software.draw_if_needed(|renderer| {
            let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; (W * H) as usize];
            renderer.render(&mut pixels, W as usize);
        });
    }

    fn touch(&self, id: i32, (x, y): (f32, f32), phase: TouchPhase) {
        self.software.dispatch_event(WindowEvent::internal(InternalEvent::Touch {
            id,
            position: (x, y).into(),
            phase,
        }));
        self.draw();
    }

    fn tap(&self, at: (f32, f32)) {
        self.touch(0, at, TouchPhase::Started);
        self.touch(0, at, TouchPhase::Ended);
    }

    fn mouse_move(&self, (x, y): (f32, f32)) {
        self.software.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(x, y) });
        self.draw();
    }

    fn click(&self, (x, y): (f32, f32)) {
        let position = LogicalPosition::new(x, y);
        self.software.dispatch_event(WindowEvent::PointerMoved { position });
        self.software.dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
        self.software.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
        self.draw();
    }

    fn back(&self) {
        let text: SharedString = Key::Back.into();
        self.software.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        self.software.dispatch_event(WindowEvent::KeyReleased { text });
        self.draw();
    }

    fn idle(&self, time: Duration) {
        self.skipped.set(self.skipped.get() + time);
        self.draw();
    }

    fn inputs(&self) -> Vec<Input> {
        std::mem::take(&mut *self.inputs.borrow_mut())
    }

    fn menu_open(&self) -> bool {
        self.adapter.window().global::<Session>().get_menu_open()
    }

    /// The menu button, when it is on screen.
    fn button(&self) -> Option<ItemRc> {
        let label = tr!("打开游戏菜单");
        let window = WindowInner::from_pub(self.software.window());
        let mut stack = vec![window.window_item_rc()?];
        while let Some(item) = stack.pop() {
            stack.extend(std::iter::successors(item.first_child(), ItemRc::next_sibling));
            if item.is_visible()
                && item.accessible_role() == AccessibleRole::Button
                && item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .is_some_and(|text| text.as_str() == label)
            {
                return Some(item);
            }
        }
        None
    }

    fn button_enabled(&self) -> bool {
        let button = self.button().expect("menu button on screen");
        button.accessible_string_property(AccessibleStringProperty::Enabled).as_deref() == Some("true")
    }

    /// Idles past the 3 s timer and, with motion on, the fade that follows it.
    #[track_caller]
    fn hide_button(&self) {
        self.idle(Duration::from_secs(4));
        self.idle(Duration::from_secs(1));
        assert!(self.button().is_none(), "{}: button still shown after idling", self.language.get());
        self.inputs();
    }
}

#[test]
fn touch_reveals_game_menu_button_without_taking_game_input() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let skipped = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
        skipped: skipped.clone(),
    }))
    .expect("software platform");

    let adapter = SlintManagerAdapter::new().expect("manager window");
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    let languages: &[&'static str] = if output.is_some() { &["zh-CN", "en", "ja"] } else { &["ja"] };
    let f = Fixture { adapter, software, skipped, inputs: inputs.clone(), output, language: Cell::new("") };
    let window = f.adapter.window();
    window.global::<MaterialWindowAdapter>().set_touch(true);
    f.adapter.apply(&model(true));
    window.global::<App>().set_page("library".into());
    let session = window.global::<Session>();
    session.set_title("Harbor Lights".into());
    session.set_frame(game_frame());
    session.set_output_aspect(W as f32 / H as f32);
    let (game_width, game_height) = (W as f32 * GAME_SCALE, H as f32 * GAME_SCALE);
    session.set_native_width(game_width);
    session.set_native_height(game_height);
    session.set_raster_width(game_width);
    session.set_raster_height(game_height);
    session.set_content_width(game_width);
    session.set_content_height(game_height);
    session.on_input(move |name, pressed, value| {
        inputs.borrow_mut().push(Input(name.to_string(), pressed, value));
    });
    f.adapter.set_session_active(true);
    window.show().expect("show");

    for &language in languages {
        i18n::apply(language);
        f.language.set(language);
        f.adapter.apply(&model(true));
        render(&f.software);
        f.hide_button();
        f.save("hidden");

        // A touch reaches the game unchanged; the button appears when it lifts.
        f.touch(0, (400.0, 200.0), TouchPhase::Started);
        assert!(f.button().is_none(), "{language}: button shown while the revealing finger is down");
        assert_inputs(f.inputs(), vec![down(400.0, 200.0)], "first touch down");
        f.touch(0, (400.0, 200.0), TouchPhase::Ended);
        assert_inputs(f.inputs(), vec![up(400.0, 200.0)], "first touch up");
        assert!(!f.menu_open(), "{language}: revealing touch opened the menu");
        let button = f.button().expect("menu button after the touch lifted");
        let rect = button.geometry();
        let centre = button.map_to_native_window(rect.center());
        assert_eq!((rect.width(), rect.height()), (48.0, 48.0), "{language}: touch button size");
        assert!((centre.x - BUTTON.0).abs() < 0.5 && (centre.y - BUTTON.1).abs() < 0.5, "{language}: button at {centre:?}");
        assert!(f.button_enabled(), "{language}: shown button not enabled");
        f.save("revealed");

        // Drags are delivered as down, moves and up; the shown button gives way while the finger is down.
        f.touch(0, (300.0, 200.0), TouchPhase::Started);
        assert!(f.button().is_none(), "{language}: button stayed shown under a game touch");
        f.touch(0, (340.0, 220.0), TouchPhase::Moved);
        f.touch(0, (380.0, 240.0), TouchPhase::Moved);
        f.touch(0, (380.0, 240.0), TouchPhase::Ended);
        assert_inputs(
            f.inputs(),
            vec![down(300.0, 200.0), pointer(340.0, 220.0), pointer(380.0, 240.0), up(380.0, 240.0)],
            "drag",
        );
        assert!(f.button().is_some(), "{language}: button not back after the drag lifted");

        // The former right-edge strip: taps and leftward swipes go straight to the game.
        f.tap((770.0, 300.0));
        assert_inputs(f.inputs(), vec![down(770.0, 300.0), up(770.0, 300.0)], "right-edge tap");
        f.touch(0, (770.0, 180.0), TouchPhase::Started);
        assert_inputs(f.inputs(), vec![down(770.0, 180.0)], "right-edge press is not withheld");
        f.touch(0, (740.0, 180.0), TouchPhase::Moved);
        f.touch(0, (700.0, 180.0), TouchPhase::Moved);
        f.touch(0, (700.0, 180.0), TouchPhase::Ended);
        assert_inputs(f.inputs(), vec![pointer(740.0, 180.0), pointer(700.0, 180.0), up(700.0, 180.0)], "right-edge swipe");
        assert!(!f.menu_open(), "{language}: right-edge swipe opened the menu");

        // A touch that lands where the hidden button sits stays with the game.
        f.hide_button();
        f.touch(0, BUTTON, TouchPhase::Started);
        assert!(f.button().is_none(), "{language}: button shown under the revealing finger");
        f.touch(0, (BUTTON.0 + 2.0, BUTTON.1 + 2.0), TouchPhase::Moved);
        f.touch(0, (BUTTON.0 + 2.0, BUTTON.1 + 2.0), TouchPhase::Ended);
        assert_inputs(
            f.inputs(),
            vec![down(BUTTON.0, BUTTON.1), pointer(BUTTON.0 + 2.0, BUTTON.1 + 2.0), up(BUTTON.0 + 2.0, BUTTON.1 + 2.0)],
            "touch on the hidden button",
        );
        assert!(!f.menu_open(), "{language}: the revealing finger opened the menu");

        // Two fingers: when one lifts, Slint re-presses the other where it
        // rests. Resting on the button, that press still belongs to the game,
        // whichever finger came first.
        for (first, second) in [((400.0, 200.0), BUTTON), (BUTTON, (400.0, 200.0))] {
            // Start hidden so the first finger is the one that reveals the button.
            f.hide_button();
            f.touch(0, first, TouchPhase::Started);
            f.touch(1, second, TouchPhase::Started);
            let (lifted, (lifted_at, resting)) = if first == BUTTON { (1, (second, first)) } else { (0, (first, second)) };
            f.touch(lifted, lifted_at, TouchPhase::Ended);
            assert!(f.button().is_none(), "{language}: button shown while a finger rests on it");
            f.touch(1 - lifted, resting, TouchPhase::Ended);
            assert_inputs(
                f.inputs(),
                vec![down(first.0, first.1), up(first.0, first.1), down(BUTTON.0, BUTTON.1), up(BUTTON.0, BUTTON.1)],
                "two-finger gesture ending on the button",
            );
            assert!(!f.menu_open(), "{language}: a finger of a game gesture opened the menu");
            assert!(f.button().is_some(), "{language}: button not shown after the gesture lifted");
        }

        // A cancelled single touch is released to the game.
        f.touch(0, (400.0, 200.0), TouchPhase::Started);
        f.touch(0, (400.0, 200.0), TouchPhase::Cancelled);
        assert_inputs(f.inputs(), vec![down(400.0, 200.0), up(400.0, 200.0)], "cancelled touch");
        assert!(f.button_enabled(), "{language}: button not usable after a cancelled touch");

        // A cancelled two-finger touch ends without a matching up. The button
        // stays hidden instead of showing without reacting; the next tap on
        // the game reveals it when it lifts, and one tap on it opens the menu.
        f.touch(0, (400.0, 200.0), TouchPhase::Started);
        f.touch(1, (600.0, 200.0), TouchPhase::Started);
        f.touch(0, (400.0, 200.0), TouchPhase::Cancelled);
        f.touch(1, (600.0, 200.0), TouchPhase::Cancelled);
        assert_inputs(f.inputs(), vec![down(400.0, 200.0), up(400.0, 200.0)], "cancelled two-finger touch");
        assert!(f.button().is_none(), "{language}: button shown after a two-finger cancel");
        f.tap((300.0, 150.0));
        assert_inputs(f.inputs(), vec![down(300.0, 150.0), up(300.0, 150.0)], "tap after a two-finger cancel");
        assert!(f.button_enabled(), "{language}: tap after a two-finger cancel did not reveal the button");
        f.tap(BUTTON);
        assert!(f.menu_open(), "{language}: first tap on the button after a two-finger cancel did not open the menu");
        assert_inputs(f.inputs(), vec![], "tap on the button after a two-finger cancel");
        f.back();

        // Only a fresh tap on the visible button opens the menu, without game input.
        f.tap((400.0, 200.0));
        f.inputs();
        assert!(f.button_enabled(), "{language}: button not shown after a tap");
        f.tap(BUTTON);
        assert!(f.menu_open(), "{language}: tap on the visible button did not open the menu");
        assert_inputs(f.inputs(), vec![], "tap on the visible button");
        assert!(f.button().is_none(), "{language}: button shown over the open menu");
        f.save("menu-open");

        // System Back closes and opens the menu as before.
        f.back();
        assert!(!f.menu_open(), "{language}: Back did not close the menu");
        f.back();
        assert!(f.menu_open(), "{language}: Back did not open the menu");
        f.back();
        assert!(!f.menu_open(), "{language}: Back did not close the menu");
        assert_inputs(f.inputs(), vec![], "Back");

        // Screen readers activate the visible button directly.
        f.tap((400.0, 200.0));
        f.inputs();
        f.button().expect("menu button").accessible_action(&AccessibilityAction::Default);
        assert!(f.menu_open(), "{language}: accessible action did not open the menu");
        assert_inputs(f.inputs(), vec![], "accessible action");
        f.back();

        // Three seconds without activity hide the button; a tap there is game input again.
        f.tap((400.0, 200.0));
        f.inputs();
        f.idle(Duration::from_millis(2500));
        assert!(f.button().is_some(), "{language}: button hid before 3 s");
        f.idle(Duration::from_millis(1000));
        assert!(f.button().is_none(), "{language}: button still shown after 3 s idle");
        f.save("idle-hidden");
        f.tap(BUTTON);
        assert_inputs(f.inputs(), vec![down(BUTTON.0, BUTTON.1), up(BUTTON.0, BUTTON.1)], "tap where the button was");
        assert!(!f.menu_open(), "{language}: hidden button opened the menu");

        // With motion on, the button fades out and takes no input while fading.
        f.adapter.apply(&model(false));
        f.hide_button();
        f.tap((400.0, 200.0));
        f.inputs();
        f.idle(Duration::from_millis(3100));
        assert!(f.button().is_some(), "{language}: fade did not start from the shown button");
        f.idle(Duration::from_millis(100));
        assert!(f.button().is_some(), "{language}: button vanished without fading");
        f.tap(BUTTON);
        assert_inputs(f.inputs(), vec![down(BUTTON.0, BUTTON.1), up(BUTTON.0, BUTTON.1)], "tap on the fading button");
        assert!(!f.menu_open(), "{language}: fading button opened the menu");
        f.idle(Duration::from_millis(3100));
        assert!(f.button().is_some(), "{language}: fade did not start");
        f.idle(Duration::from_millis(300));
        assert!(f.button().is_none(), "{language}: button still shown after the fade");
        f.adapter.apply(&model(true));

        // Mouse: movement reveals the button without game input, hover keeps it, a click opens the menu.
        f.mouse_move((400.0, 200.0));
        assert!(f.button().is_some(), "{language}: mouse movement did not reveal the button");
        f.mouse_move(BUTTON);
        f.idle(Duration::from_secs(4));
        assert!(f.button().is_some(), "{language}: hovered button hid");
        f.click(BUTTON);
        assert!(f.menu_open(), "{language}: mouse click on the button did not open the menu");
        assert_inputs(f.inputs(), vec![], "mouse reveal and click on the button");
        f.back();
        f.click((400.0, 200.0));
        assert_inputs(f.inputs(), vec![down(400.0, 200.0), up(400.0, 200.0)], "mouse click on the game");
        f.mouse_move((0.0, 0.0));
        f.inputs();

        // While the game is opening or a modal dialog is up, touches neither
        // reach the game nor open the menu; the opening screen hides the button.
        f.hide_button();
        session.set_phase("opening".into());
        f.tap((400.0, 200.0));
        assert!(f.button().is_none(), "{language}: button shown on the opening screen");
        assert_inputs(f.inputs(), vec![], "touch while opening");
        session.set_phase("running".into());
        let dialogs = window.global::<Dialogs>();
        dialogs.set_modal_active(true);
        f.tap((400.0, 200.0));
        assert_inputs(f.inputs(), vec![], "touch under a modal dialog");
        assert!(!f.menu_open(), "{language}: touch under a modal dialog opened the menu");
        dialogs.set_modal_active(false);
    }
    f.adapter.set_session_active(false);
    f.adapter.window().hide().expect("hide");
}
