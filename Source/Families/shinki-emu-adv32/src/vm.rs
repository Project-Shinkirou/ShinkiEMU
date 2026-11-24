#[cfg(feature = "video-ffmpeg")]
use crate::movie::Movie;
use crate::{
    Error, Result,
    animation::{Animation, StaffRoll},
    archive::Archive,
    audio::Audio,
    binary::decode_japanese,
    comments::Comments,
    crx::Crx,
    effects::{
        FallingSprites, LensFlare, LensPosition, ParticleKind, ParticleRegion, SceneLayer,
        SceneTransition, ScreenBreak, rand15,
    },
    game::{Game, SoundNames, read_bounded},
    graphics::{self, Clip, GraphicsCache},
    mes::{Instruction, Operands, Script},
    message::{self, Command as MessageCommand, Message},
    native_data::{
        GalleryProfile, MUSIC_TRACK_LIMIT, choice_captions, credit_slides, gallery_profile,
        music_playlist,
    },
    render::Canvas,
    save::{NativeLayer, NativeSlot, PlayerNames, Saves},
    text::{Text, TextPainter},
};
use image::RgbaImage;
use roukaku_media_core::OwnedPixelBuffer;
use shinki_emu_sdk::runtime::{KeyCode, KeyState, PointerButton, RuntimeEvent, WindowCommand};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TICK_BUDGET: usize = 8192;
const MAX_SURFACES: usize = 64;
const COMPACT_MESSAGE_BANK: u8 = 1;
const EXPANDED_MESSAGE_BANK: u8 = 6;
const MESSAGE_MACROS: usize = 10;
const BUTTON_SOUND_CHANNEL: u8 = 6;

struct TextureSlots {
    actor: u8,
    actor_count: u8,
    accessory: u8,
    panel: u8,
    cut_in: u8,
    mask: u8,
    fog: u8,
    falling: u8,
    rain: u8,
    plane: u8,
}

#[derive(Clone)]
struct MessageMacro {
    script: Arc<Script>,
    start: usize,
    end: usize,
}

#[derive(Clone)]
struct Sprite {
    source: Arc<Crx>,
    x: i32,
    y: i32,
    clip: Option<Clip>,
    use_window_alpha: bool,
    effect: Option<SpriteEffect>,
}

#[derive(Clone)]
enum SpriteEffect {
    Pulse {
        amplitude: (i32, i32),
        period: Duration,
        elapsed: Duration,
        repeat: bool,
    },
    Entrance {
        dx: i32,
        dy: i32,
        remaining: Duration,
    },
}

const SPRITE_ENTRANCE: Duration = Duration::from_millis(500);

#[derive(Default)]
struct Template {
    sprites: Vec<Sprite>,
    text: Vec<Text>,
}

#[derive(Default)]
struct Actor {
    layer: SceneLayer,
    motion: Option<Pan>,
    next_motion: VecDeque<Pan>,
    shake: Option<Shake>,
    wave: Option<Wave>,
    relative_origin: Option<(i32, i32)>,
}

struct Fade {
    total: Duration,
    remaining: Duration,
    color: [u8; 3],
    mask: Option<(Arc<Crx>, bool)>,
    pulse: bool,
}

impl Fade {
    const FLASH_PERIOD: Duration = Duration::from_micros(10 * 1_000_000 / 60);

    fn opacity(&self) -> f64 {
        if self.pulse {
            let elapsed = self.total - self.remaining;
            let phase = elapsed.div_duration_f64(Self::FLASH_PERIOD).fract();
            1.0 - (2.0 * phase - 1.0).abs()
        } else {
            self.remaining.div_duration_f64(self.total)
        }
    }
}

#[derive(Clone, Copy)]
struct Camera {
    scale: u32,
    center: (i32, i32),
}

struct CameraMotion {
    from: Camera,
    to: Camera,
    total: Duration,
    remaining: Duration,
}

struct Pan {
    from: (i32, i32),
    to: (i32, i32),
    remaining: Duration,
    total: Duration,
}

impl Pan {
    fn advance(&mut self, elapsed: Duration) -> (i32, i32) {
        self.remaining = self.remaining.saturating_sub(elapsed);
        let fraction = 1.0 - self.remaining.div_duration_f64(self.total);
        let approach =
            |start: i32, end: i32| start + (f64::from(end - start) * fraction).round() as i32;
        (
            approach(self.from.0, self.to.0),
            approach(self.from.1, self.to.1),
        )
    }
}

#[derive(Default)]
struct PanningPlane {
    position: [i32; 2],
    motions: [Option<Pan>; 2],
}

struct Shake {
    elapsed: Duration,
    vertical: bool,
    alternate: bool,
}

struct Wave {
    elapsed: Duration,
    duration: Duration,
    amplitude: f64,
    angles: (f64, f64),
    vertical: bool,
}

impl Wave {
    fn offset(&self) -> (i32, i32) {
        let progress = self.elapsed.div_duration_f64(self.duration).min(1.0);
        let angle = self.angles.0 + (self.angles.1 - self.angles.0) * progress;
        let amount = (self.amplitude * angle.to_radians().sin()).round() as i32;
        if self.vertical {
            (0, amount)
        } else {
            (amount, 0)
        }
    }
}

impl Shake {
    const SEGMENT: Duration = Duration::from_nanos(166_666_667);

    fn offset(&self) -> (i32, i32) {
        let phase = self.elapsed.div_duration_f64(Self::SEGMENT);
        let third = if self.alternate { -8 } else { 8 };
        let offsets = [0, 8, 0, third, 0];
        let index = (phase as usize).min(3);
        let fraction = (phase - index as f64).clamp(0.0, 1.0);
        let offset = offsets[index]
            + (f64::from(offsets[index + 1] - offsets[index]) * fraction).round() as i32;
        if self.vertical {
            (0, offset)
        } else {
            (offset, 0)
        }
    }

    fn advance(slot: &mut Option<Self>, elapsed: Duration) -> bool {
        let Some(shake) = slot else { return false };
        shake.elapsed += elapsed;
        if shake.elapsed >= Self::SEGMENT * 4 {
            *slot = None;
        }
        true
    }
}

#[derive(Clone, Copy)]
struct Slider {
    start: i32,
    end: i32,
    last_value: i32,
    binding: (u16, u16),
}

impl Slider {
    fn thumb_x(self, value: i32) -> i32 {
        self.start + (self.end - self.start) * value.clamp(0, self.last_value) / self.last_value
    }

    fn value_at(self, x: i32) -> i32 {
        let position = (x - self.start) as f64 / (self.end - self.start) as f64;
        (position * f64::from(self.last_value))
            .round()
            .clamp(0.0, f64::from(self.last_value)) as i32
    }
}

#[derive(Default)]
struct MouseGesture {
    held: Duration,
    origin: Option<(i32, i32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SliderDrag {
    button: usize,
    value: i32,
}

#[derive(Clone, Copy, Default)]
struct ButtonDrag {
    source: bool,
    target: bool,
}

#[derive(Clone, Default)]
struct Button {
    area: [i32; 4],
    label: Vec<u8>,
    states: BTreeMap<u16, Vec<Sprite>>,
    captions: BTreeMap<u16, Vec<Text>>,
    slider: Option<Slider>,
    sounds: [u16; 2],
    drag: ButtonDrag,
    hit_test_alpha: bool,
    music_slider: bool,
    draw_on_top: bool,
    save_slot: Option<u16>,
    list_index: Option<u16>,
}

impl Button {
    fn selectable(&self) -> bool {
        !self.drag.source && !self.label.is_empty() && self.label != b":n"
    }

    fn hit_at(&self, x: i32, y: i32) -> bool {
        let [left, top, right, bottom] = self.area;
        if !(left..right).contains(&x) || !(top..bottom).contains(&y) {
            return false;
        }
        if !self.hit_test_alpha {
            return true;
        }
        let x = x - left;
        let y = y - top;
        if x == 0 || y == 0 {
            return false;
        }
        self.states
            .get(&99)
            .and_then(|state| state.first())
            .and_then(|sprite| {
                let (left, top) = sprite.clip.map_or((0, 0), |clip| (clip.x, clip.y));
                let x = u32::try_from(left + x).ok()?;
                let y = u32::try_from(top + y).ok()?;
                sprite.source.image.get_pixel_checked(x, y)
            })
            .is_some_and(|pixel| pixel[3] != 0)
    }
}

struct MessageButtonBank {
    buttons: Vec<Button>,
    artwork: Template,
}

struct ScrollList {
    bounds: [i32; 4],
    items: Vec<Button>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wait {
    Ready,
    Input,
    Menu,
    Timer {
        remaining: Duration,
        skippable: bool,
    },
    Audio {
        flags: u8,
        start_timeout: Duration,
    },
    VoiceTail(Duration),
    NameEntry,
    Motion,
    Credits,
    #[cfg(feature = "video-ffmpeg")]
    Movie,
    Finished,
}

#[derive(Clone, Copy)]
enum QuickOperation {
    Save,
    Load,
}

#[derive(Clone, Copy)]
enum Confirmation {
    Remember(u16),
    Prompt,
}

struct QuickPrompt {
    operation: QuickOperation,
    save: Option<(NativeSlot, RgbaImage)>,
    button_group: u8,
    origin: (i32, i32),
}

struct BacklogEntry {
    speaker: String,
    line: String,
    voice: Option<String>,
    checkpoint: Option<Box<NativeSlot>>,
}

struct LogLayout {
    rows: usize,
    row_pitch: i32,
    atlas: u8,
    back: [i32; 4],
    scrollbar: [i32; 4],
}

impl LogLayout {
    fn for_dialect(classic: bool) -> Self {
        if classic {
            Self {
                rows: 5,
                row_pitch: 125,
                atlas: 14,
                back: [1234, 672, 1282, 716],
                scrollbar: [1114, 84, 1146, 692],
            }
        } else {
            Self {
                rows: 4,
                row_pitch: 150,
                atlas: 19,
                back: [1100, 672, 1216, 712],
                scrollbar: [1232, 82, 1256, 664],
            }
        }
    }

    fn jump_area(&self, classic: bool, row: usize) -> [i32; 4] {
        let y = row as i32 * self.row_pitch;
        if classic {
            [8, y + 118, 162, y + 164]
        } else {
            [328, y + 110, 488, y + 144]
        }
    }

    fn voice_area(&self, classic: bool, row: usize) -> [i32; 4] {
        let y = row as i32 * self.row_pitch;
        if classic {
            [288, y + 76, 1064, y + 186]
        } else {
            [328, y + 150, 488, y + 183]
        }
    }

    fn contains([left, top, right, bottom]: [i32; 4], (x, y): (i32, i32)) -> bool {
        (left..right).contains(&x) && (top..bottom).contains(&y)
    }
}

struct ScriptMenu {
    story: NativeSlot,
    thumbnail: RgbaImage,
    return_wait: Wait,
    buttons: BTreeMap<u8, Vec<Button>>,
    group: u8,
    choices: Option<Vec<Button>>,
}

struct NameEntry {
    names: PlayerNames,
    given_selected: bool,
    backdrop: Option<Arc<Crx>>,
}

pub(crate) struct Vm {
    game: Game,
    #[cfg(not(target_env = "ohos"))]
    gpu: Option<crate::render::GpuCompositor>,
    text_encoding: &'static encoding_rs::Encoding,
    script: Arc<Script>,
    script_name: String,
    next_label: Option<Vec<u8>>,
    ip: usize,
    extent: (u32, u32),
    texture_slots: TextureSlots,
    saves: Saves,
    player_names: PlayerNames,
    custom_names: bool,
    name_entry: Option<NameEntry>,
    sound_names: SoundNames,
    audio: Option<Audio>,
    next_effect_channel: u8,
    background_music: Option<u16>,
    music_volume: Option<f32>,
    saved_music_volume: Option<f32>,
    music_end_route: Option<Vec<u8>>,
    music_idle: Duration,
    music_tracks: Vec<u16>,
    music_titles: Vec<String>,
    music_order: Vec<u16>,
    sound_pan: [f32; 3],
    sound_pan_prefixes: Vec<(String, f32)>,
    variables: BTreeMap<(u16, u16), i32>,
    graphics: GraphicsCache,
    surfaces: BTreeMap<u8, Arc<Crx>>,
    surface_names: BTreeMap<u8, String>,
    previous_accessories: BTreeMap<u8, Vec<u8>>,
    animation_sources: BTreeMap<u8, Arc<Archive>>,
    animation: Option<Animation>,
    animation_position: (i32, i32),
    staff_roll: Option<StaffRoll>,
    #[cfg(feature = "video-ffmpeg")]
    movie: Option<Movie>,
    palette: BTreeMap<u16, [u8; 3]>,
    parties: BTreeMap<u8, Vec<Button>>,
    choices: Option<Vec<Button>>,
    classic_choice_text: BTreeMap<Vec<u8>, String>,
    templates: BTreeMap<u8, Template>,
    message_button_banks: BTreeMap<u8, MessageButtonBank>,
    active_message_bank: u8,
    button_group: u8,
    button_highlight: bool,
    button_options: u16,
    button_routes: BTreeMap<u8, Vec<u8>>,
    menu_layers_enabled: bool,
    slot_rectangles: Vec<[i32; 4]>,
    scroll_list: Option<ScrollList>,
    slot_thumbnails: BTreeMap<u16, Arc<Crx>>,
    menu_position: (i32, i32, u16),
    confirmation: Option<Confirmation>,
    quick_prompt: Option<QuickPrompt>,
    menu_variant: Option<u16>,
    active_menu_origin: (i32, i32),
    menu_pan: Option<Pan>,
    keyboard_selection: Option<usize>,
    slider_drag: Option<SliderDrag>,
    dragged_button: Option<usize>,
    control_keys: u8,
    skip_dialogue: bool,
    auto_dialogue: bool,
    hidden_message: bool,
    backlog: VecDeque<BacklogEntry>,
    backlog_offset: Option<usize>,
    script_menu: Option<ScriptMenu>,
    last_voice: Option<String>,
    pending_voice: Option<String>,
    pending_log_entry: bool,
    background: Option<Arc<Crx>>,
    background_position: (i32, i32),
    camera: Camera,
    camera_duration: u16,
    camera_motion: Option<CameraMotion>,
    scene_translation: (i32, i32),
    scene_translation_motion: Option<Pan>,
    preserve_view: bool,
    center_next_background: bool,
    centered_background: bool,
    fitted_background: Option<(u32, u32)>,
    panning_planes: BTreeMap<u8, PanningPlane>,
    pan: Option<Pan>,
    scene_shake: Option<Shake>,
    recording_motion: bool,
    recorded_motion: Vec<(Arc<Script>, usize)>,
    fade_color: [u8; 3],
    fade: Option<Fade>,
    mosaic: u32,
    mosaic_tween: Option<Pan>,
    falling: Option<FallingSprites>,
    particle_region: ParticleRegion,
    lens: Option<LensFlare>,
    lens_position: LensPosition,
    fog_enabled: bool,
    transition: Option<SceneTransition>,
    transition_backdrop: Option<OwnedPixelBuffer>,
    last_frame: Option<OwnedPixelBuffer>,
    screen_break: Option<ScreenBreak>,
    cut_in: SceneLayer,
    overlays: Vec<Sprite>,
    overlay_text: Vec<Text>,
    gallery: Option<GalleryProfile>,
    gallery_portrait: Option<Arc<Crx>>,
    text_painter: TextPainter,
    blit_offset: (i32, i32),
    actors: BTreeMap<u16, Actor>,
    actor_wave_frames: u16,
    actor_wave_vertical: bool,
    actor_anchor: Option<(u16, i32, i32)>,
    actor_anchors: BTreeMap<(u16, String), (i32, i32)>,
    resume_offset: Option<usize>,
    pointer: (i32, i32),
    mouse_gesture: Option<MouseGesture>,
    message: Option<Message>,
    message_offset: Option<usize>,
    message_panel: Option<Clip>,
    message_macros: [Option<MessageMacro>; MESSAGE_MACROS],
    message_macro_slot: usize,
    speaker: String,
    metadata: String,
    tick_label: u16,
    tick_step: u16,
    cursor: Option<String>,
    comments: Option<Comments>,
    voice_flags: Vec<String>,
    texture_variant: u16,
    input_mode: u8,
    skip_disc_check: bool,
    #[cfg(not(target_env = "ohos"))]
    null_audio: bool,
    display_preference: u8,
    window: VecDeque<WindowCommand>,
    fullscreen: bool,
    random: u32,
    frame_dirty: bool,
    wait: Wait,
}

impl Vm {
    pub(crate) fn new(game: Game, font_file: Option<&Path>) -> Result<Self> {
        let script = Arc::clone(&game.startup);
        let extent = match script.dialect.version {
            // Logical extent used by this engine dialect's display transform.
            0x9e97 | 0x9f9a | 0xa6b9 | 0xaab6 | 0xabb6 => (1280, 720),
            version => return Err(Error::UnsupportedMesVersion(version)),
        };
        let text_encoding = game.text_encoding();
        let root = game
            .data
            .parent()
            .ok_or(Error::Invalid("ADV32 game directory"))?;
        let font = if let Some(path) = font_file {
            Some(crate::text::configured_font(root, path)?)
        } else {
            crate::text::native_font(root).ok().flatten()
        };
        let saves = Saves::open(&game.data, script.dialect.version, text_encoding)?;
        let player_names = saves.player_names()?;
        let sound_names = game.sound_names()?;
        let texture_slots = if script.dialect.classic() {
            TextureSlots {
                actor: 2,
                actor_count: 4,
                accessory: 6,
                panel: 26,
                cut_in: 19,
                mask: 16,
                fog: 23,
                falling: 20,
                rain: 21,
                plane: 16,
            }
        } else {
            TextureSlots {
                actor: 4,
                actor_count: 5,
                accessory: 9,
                panel: 37,
                cut_in: 25,
                mask: 21,
                fog: 26,
                falling: 28,
                rain: 29,
                plane: 21,
            }
        };
        let ip = script.entry(0)?;
        Ok(Self {
            game,
            #[cfg(not(target_env = "ohos"))]
            gpu: None,
            text_encoding,
            script,
            script_name: "start".into(),
            next_label: None,
            ip,
            extent,
            texture_slots,
            saves,
            player_names,
            custom_names: false,
            name_entry: None,
            sound_names,
            audio: None,
            next_effect_channel: 2,
            background_music: None,
            music_volume: None,
            saved_music_volume: None,
            music_end_route: None,
            music_idle: Duration::ZERO,
            music_tracks: Vec::new(),
            music_titles: Vec::new(),
            music_order: Vec::new(),
            sound_pan: [0.0; 3],
            sound_pan_prefixes: Vec::new(),
            variables: BTreeMap::new(),
            graphics: GraphicsCache::default(),
            surfaces: BTreeMap::new(),
            surface_names: BTreeMap::new(),
            previous_accessories: BTreeMap::new(),
            animation_sources: BTreeMap::new(),
            animation: None,
            animation_position: (0, 0),
            staff_roll: None,
            #[cfg(feature = "video-ffmpeg")]
            movie: None,
            palette: BTreeMap::new(),
            parties: BTreeMap::new(),
            choices: None,
            classic_choice_text: BTreeMap::new(),
            templates: BTreeMap::new(),
            message_button_banks: BTreeMap::new(),
            active_message_bank: EXPANDED_MESSAGE_BANK,
            button_group: 0,
            button_highlight: true,
            button_options: 0,
            button_routes: BTreeMap::new(),
            menu_layers_enabled: false,
            slot_rectangles: Vec::new(),
            scroll_list: None,
            slot_thumbnails: BTreeMap::new(),
            menu_position: (0, 0, 0),
            confirmation: None,
            quick_prompt: None,
            menu_variant: None,
            active_menu_origin: (0, 0),
            menu_pan: None,
            keyboard_selection: None,
            slider_drag: None,
            dragged_button: None,
            control_keys: 0,
            skip_dialogue: false,
            auto_dialogue: false,
            hidden_message: false,
            backlog: VecDeque::new(),
            backlog_offset: None,
            script_menu: None,
            last_voice: None,
            pending_voice: None,
            pending_log_entry: false,
            background: None,
            background_position: (0, 0),
            camera: Camera {
                scale: 100,
                center: (0, 0),
            },
            camera_duration: 0,
            camera_motion: None,
            scene_translation: (0, 0),
            scene_translation_motion: None,
            preserve_view: false,
            center_next_background: false,
            centered_background: false,
            fitted_background: None,
            panning_planes: BTreeMap::new(),
            pan: None,
            scene_shake: None,
            recording_motion: false,
            recorded_motion: Vec::new(),
            fade_color: [0; 3],
            fade: None,
            mosaic: 0,
            mosaic_tween: None,
            falling: None,
            particle_region: ParticleRegion::default(),
            lens: None,
            lens_position: LensPosition::default(),
            fog_enabled: false,
            transition: None,
            transition_backdrop: None,
            last_frame: None,
            screen_break: None,
            cut_in: SceneLayer::default(),
            overlays: Vec::new(),
            overlay_text: Vec::new(),
            gallery: None,
            gallery_portrait: None,
            text_painter: TextPainter::new(text_encoding, font),
            blit_offset: (0, 0),
            actors: BTreeMap::new(),
            actor_wave_frames: 60,
            actor_wave_vertical: false,
            actor_anchor: None,
            actor_anchors: BTreeMap::new(),
            resume_offset: None,
            pointer: (-1, -1),
            mouse_gesture: None,
            message: None,
            message_offset: None,
            message_panel: None,
            message_macros: Default::default(),
            message_macro_slot: 0,
            speaker: String::new(),
            metadata: String::new(),
            tick_label: 0,
            tick_step: 0,
            cursor: None,
            comments: None,
            voice_flags: Vec::new(),
            texture_variant: 0,
            input_mode: 0,
            skip_disc_check: false,
            #[cfg(not(target_env = "ohos"))]
            null_audio: false,
            display_preference: 0,
            window: VecDeque::new(),
            fullscreen: false,
            random: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u32,
            frame_dirty: false,
            wait: Wait::Ready,
        })
    }

    pub(crate) fn extent(&self) -> (u32, u32) {
        self.extent
    }

    pub(crate) fn finished(&self) -> bool {
        self.wait == Wait::Finished
    }

    pub(crate) fn set_paused(&mut self, paused: bool) {
        if let Some(audio) = &self.audio {
            audio.set_paused(paused);
        }
    }

    pub(crate) fn set_skip_disc_check(&mut self, skip: bool) {
        self.skip_disc_check = skip;
    }

    #[cfg(not(target_env = "ohos"))]
    pub(crate) fn set_null_audio(&mut self, enabled: bool) {
        self.null_audio = enabled;
    }

    fn ensure_audio(&mut self) -> Result<()> {
        if self.audio.is_none() {
            self.audio = Some(Audio::open(
                #[cfg(not(target_env = "ohos"))]
                self.null_audio,
            )?);
        }
        Ok(())
    }

    pub(crate) fn window_command(&mut self) -> Option<WindowCommand> {
        self.window.pop_front()
    }

    fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
        self.window
            .push_back(WindowCommand::SetFullscreen(fullscreen));
    }

    pub(crate) fn flush_pending_saves(&mut self) -> Result<()> {
        self.saves.flush_pending()
    }

    pub(crate) fn tick(
        &mut self,
        elapsed: Duration,
        events: &[RuntimeEvent],
    ) -> Result<Option<OwnedPixelBuffer>> {
        #[cfg(not(target_env = "ohos"))]
        let previous_wait = std::mem::discriminant(&self.wait);
        self.advance_timeline(elapsed, events)?;
        #[cfg(not(target_env = "ohos"))]
        let frame = if self.gpu.is_some() {
            // Input, first frames and wait transitions are presented immediately.
            // A continuing animation can have one GPU frame in flight.
            let defer = elapsed != Duration::ZERO
                && events.is_empty()
                && previous_wait == std::mem::discriminant(&self.wait);
            self.render_tick(defer)?
        } else {
            self.render_raster_tick()?
        };
        #[cfg(target_env = "ohos")]
        let frame = self.render_raster_tick()?;
        if let Some(pixels) = &frame {
            self.last_frame = Some(pixels.clone());
        }
        Ok(frame)
    }

    fn render_raster_tick(&mut self) -> Result<Option<OwnedPixelBuffer>> {
        if std::mem::take(&mut self.frame_dirty) {
            Ok(Some(self.compose()?.into_raw().into()))
        } else {
            Ok(None)
        }
    }

    #[cfg(not(target_env = "ohos"))]
    fn render_tick(&mut self, defer: bool) -> Result<Option<OwnedPixelBuffer>> {
        let ready = self
            .gpu
            .as_mut()
            .expect("GPU compositor is enabled")
            .poll()?;
        if !self.frame_dirty {
            return Ok(ready);
        }
        let use_gpu = !(1..1000).contains(&self.mosaic)
            && self.fade.as_ref().is_none_or(|fade| fade.mask.is_none())
            && self.screen_break.is_none()
            && !self.cut_in.has_mask()
            && !self.actors.values().any(|actor| actor.layer.has_mask());
        #[cfg(feature = "video-ffmpeg")]
        let use_gpu = use_gpu && self.movie.is_none();
        if use_gpu
            && defer
            && self
                .gpu
                .as_ref()
                .expect("GPU compositor is enabled")
                .is_busy()
        {
            return Ok(ready);
        }
        let canvas = self.paint(use_gpu)?;
        self.frame_dirty = false;
        let compositor = self.gpu.as_mut().expect("GPU compositor is enabled");
        match canvas {
            Canvas::Raster(image) => {
                compositor.discard_pending();
                Ok(Some(image.into_raw().into()))
            }
            Canvas::Scene(scene) if defer => {
                compositor.submit(scene)?;
                Ok(ready)
            }
            Canvas::Scene(scene) => compositor.render_now(scene).map(Some),
        }
    }

    #[cfg(not(target_env = "ohos"))]
    pub(crate) fn enable_hardware_compositor(&mut self) -> Result<()> {
        self.gpu = Some(crate::render::GpuCompositor::new(
            self.text_encoding,
            self.text_painter.font(),
        )?);
        Ok(())
    }

    fn advance_name_entry(&mut self, events: &[RuntimeEvent]) -> Result<()> {
        let Some(entry) = &mut self.name_entry else {
            return Ok(());
        };
        let mut accept = None;
        for event in events {
            match event {
                RuntimeEvent::PointerMove { x, y } => self.pointer = (*x as i32, *y as i32),
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Released,
                } => {
                    let left = (self.extent.0 as i32 - 500) / 2;
                    let top = (self.extent.1 as i32 - 220) / 2;
                    let (x, y) = self.pointer;
                    if (left..left + 500).contains(&x) && (top..top + 120).contains(&y) {
                        entry.given_selected = y >= top + 60;
                        self.frame_dirty = true;
                    }
                }
                RuntimeEvent::Key {
                    code: KeyCode::Tab | KeyCode::ArrowUp | KeyCode::ArrowDown,
                    state: KeyState::Pressed,
                    ..
                } => {
                    entry.given_selected = !entry.given_selected;
                    self.frame_dirty = true;
                }
                RuntimeEvent::Key {
                    code: KeyCode::Enter | KeyCode::Escape,
                    state: KeyState::Pressed,
                    ..
                } => {
                    accept = Some(matches!(
                        event,
                        RuntimeEvent::Key {
                            code: KeyCode::Enter,
                            ..
                        }
                    ));
                    break;
                }
                RuntimeEvent::Key {
                    code: KeyCode::Backspace,
                    state: KeyState::Pressed,
                    ..
                } => {
                    let field = if entry.given_selected {
                        &mut entry.names.given
                    } else {
                        &mut entry.names.family
                    };
                    field.pop();
                    self.frame_dirty = true;
                }
                RuntimeEvent::TextInput { text } if !text.chars().any(char::is_control) => {
                    let field = if entry.given_selected {
                        &mut entry.names.given
                    } else {
                        &mut entry.names.family
                    };
                    let mut edited = field.clone();
                    edited.push_str(text);
                    let (bytes, _, invalid) = self.saves.player_name_encoding()?.encode(&edited);
                    if !invalid && bytes.len() < 16 {
                        *field = edited;
                        self.frame_dirty = true;
                    }
                }
                _ => {}
            }
        }
        if let Some(accept) = accept {
            if accept {
                self.saves.set_player_names(&entry.names)?;
                self.player_names = entry.names.clone();
            }
            self.name_entry = None;
            self.wait = Wait::Ready;
            self.frame_dirty = true;
        }
        Ok(())
    }

    fn advance_timeline(&mut self, elapsed: Duration, events: &[RuntimeEvent]) -> Result<()> {
        if let Some(audio) = &mut self.audio {
            audio.check()?;
        }
        if self.name_entry.is_some() {
            return self.advance_name_entry(events);
        }
        if self
            .staff_roll
            .as_ref()
            .is_some_and(StaffRoll::needs_followup_music)
            && !self.audio.as_ref().is_some_and(|audio| audio.playing(0))
        {
            self.play_music_track(46, false)?;
            if let Some(roll) = &mut self.staff_roll {
                roll.start_followup();
            }
        }
        let (elapsed, events) = if let Some(roll) = &mut self.staff_roll {
            let dismiss = events.iter().any(|event| {
                matches!(
                    event,
                    RuntimeEvent::PointerButton {
                        button: PointerButton::Secondary,
                        state: KeyState::Released,
                    } | RuntimeEvent::Key {
                        code: KeyCode::Escape,
                        state: KeyState::Pressed,
                        ..
                    }
                )
            });
            let confirm = events.iter().any(|event| {
                matches!(
                    event,
                    RuntimeEvent::PointerButton {
                        button: PointerButton::Primary,
                        state: KeyState::Released,
                    } | RuntimeEvent::Key {
                        code: KeyCode::Enter | KeyCode::Space,
                        state: KeyState::Pressed,
                        ..
                    }
                )
            });
            let read_flag = if roll.route_slides() { 110 } else { 111 };
            let watched = self.saves.global(100, read_flag).unwrap_or(0) != 0;
            let skip = (dismiss && watched) || ((dismiss || confirm) && roll.holding());
            self.frame_dirty = true;
            if !roll.advance(elapsed, skip)? {
                return Ok(());
            }
            self.staff_roll = None;
            if self.script.dialect.version != 0x9f9a {
                self.set_variable(100, read_flag, 1)?;
            }
            self.wait = Wait::Ready;
            (Duration::ZERO, &[][..])
        } else {
            (elapsed, events)
        };
        #[cfg(feature = "video-ffmpeg")]
        if let Some(movie) = &mut self.movie {
            let audio = self
                .audio
                .as_ref()
                .ok_or(Error::Invalid("ADV32 movie audio device"))?;
            self.frame_dirty |= movie.advance(audio, elapsed)?;
            if movie.ended() {
                self.stop_movie()?;
            }
        }
        let speed = self.variable(99, 26)?.clamp(0, 11) as u32;
        if let Some(message) = &mut self.message {
            self.frame_dirty |=
                message.advance(elapsed, speed, self.script.dialect, self.text_encoding);
        }
        if let Some(comments) = &mut self.comments {
            self.frame_dirty |= comments.advance(elapsed, &mut self.random);
        }
        if let Some(animation) = &mut self.animation {
            self.frame_dirty |= animation.advance(elapsed)?;
        }
        if let Some(motion) = &mut self.camera_motion {
            motion.remaining = motion.remaining.saturating_sub(elapsed);
            let fraction = 1.0 - motion.remaining.div_duration_f64(motion.total);
            let tween = |a: i32, b: i32| a + ((b - a) as f64 * fraction).round() as i32;
            self.camera = Camera {
                scale: tween(motion.from.scale as i32, motion.to.scale as i32) as u32,
                center: (
                    tween(motion.from.center.0, motion.to.center.0),
                    tween(motion.from.center.1, motion.to.center.1),
                ),
            };
            self.frame_dirty = true;
            if motion.remaining.is_zero() {
                self.camera_motion = None;
            }
        }
        if let Some(motion) = &mut self.scene_translation_motion {
            let offset = motion.advance(elapsed);
            self.frame_dirty |= offset != self.scene_translation;
            self.scene_translation = offset;
            if motion.remaining.is_zero() {
                self.scene_translation_motion = None;
            }
        }
        if let Some(falling) = &mut self.falling {
            self.frame_dirty |= falling.update(elapsed, &mut self.random);
        }
        if let Some(lens) = &mut self.lens {
            self.frame_dirty |= lens.advance(elapsed);
        }
        if let Some(transition) = &mut self.transition {
            self.frame_dirty = true;
            if transition.finished(elapsed) {
                self.transition = None;
            }
        }
        if let Some(effect) = &mut self.screen_break {
            self.frame_dirty = true;
            if effect.finished(elapsed, self.extent) {
                self.screen_break = None;
            }
        }
        self.frame_dirty |= self.cut_in.advance(elapsed);
        if let Some(fade) = &mut self.fade {
            fade.remaining = fade.remaining.saturating_sub(elapsed);
            self.frame_dirty |= self.background.is_some();
            if fade.remaining.is_zero() {
                self.fade = None;
            }
        }
        if let Some(pan) = &mut self.pan {
            self.background_position = pan.advance(elapsed);
            self.frame_dirty = true;
            if pan.remaining.is_zero() {
                self.pan = None;
            }
        }
        for plane in self.panning_planes.values_mut() {
            for (position, motion) in plane.position.iter_mut().zip(&mut plane.motions) {
                if let Some(pan) = motion {
                    *position = pan.advance(elapsed).0;
                    self.frame_dirty = true;
                    if pan.remaining.is_zero() {
                        *motion = None;
                    }
                }
            }
        }
        if let Some(tween) = &mut self.mosaic_tween {
            self.mosaic = tween.advance(elapsed).0 as u32;
            self.frame_dirty = true;
            if tween.remaining.is_zero() {
                self.mosaic_tween = None;
            }
        }
        if let Some(pan) = &mut self.menu_pan {
            self.active_menu_origin = pan.advance(elapsed);
            self.frame_dirty = true;
            if pan.remaining.is_zero() {
                self.menu_pan = None;
            }
        }
        for actor in self.actors.values_mut() {
            self.frame_dirty |= Shake::advance(&mut actor.shake, elapsed) && actor.layer.visible;
            self.frame_dirty |= actor.layer.advance(elapsed);
            if let Some(wave) = &mut actor.wave {
                wave.elapsed = wave.elapsed.saturating_add(elapsed);
                if wave.elapsed >= wave.duration {
                    actor.wave = None;
                }
                self.frame_dirty |= actor.layer.visible;
            }
            let mut time = elapsed;
            while let Some(motion) = &mut actor.motion {
                let step = time.min(motion.remaining);
                actor.layer.position = motion.advance(step);
                self.frame_dirty |= actor.layer.visible;
                if !motion.remaining.is_zero() {
                    break;
                }
                actor.motion = actor.next_motion.pop_front();
                time -= step;
            }
        }
        self.frame_dirty |= Shake::advance(&mut self.scene_shake, elapsed);
        for sprite in &mut self.overlays {
            match &mut sprite.effect {
                Some(SpriteEffect::Entrance { remaining, .. }) => {
                    *remaining = remaining.saturating_sub(elapsed);
                    self.frame_dirty = true;
                    if remaining.is_zero() {
                        sprite.effect = None;
                    }
                }
                Some(SpriteEffect::Pulse {
                    elapsed: time,
                    period,
                    repeat,
                    ..
                }) => {
                    *time = time.saturating_add(elapsed);
                    if *time >= *period && !*repeat {
                        sprite.effect = None;
                    }
                    self.frame_dirty = true;
                }
                None => {}
            }
        }
        self.advance_mouse_gesture(elapsed)?;
        for event in events {
            match event {
                RuntimeEvent::WindowFocused { focused: false } => {
                    self.control_keys = 0;
                    self.frame_dirty |= self.slider_drag.take().is_some();
                    self.frame_dirty |= self.dragged_button.take().is_some();
                    self.frame_dirty |= self.mouse_gesture.take().is_some();
                }
                RuntimeEvent::Key {
                    code: code @ (KeyCode::ControlLeft | KeyCode::ControlRight),
                    state,
                    ..
                } => {
                    let key = if *code == KeyCode::ControlLeft { 1 } else { 2 };
                    if *state == KeyState::Pressed {
                        self.control_keys |= key;
                    } else {
                        self.control_keys &= !key;
                    }
                }
                RuntimeEvent::PointerMove { x, y } => {
                    let before = (self.hovered(), self.hovered_message_button());
                    self.pointer = (*x as i32, *y as i32);
                    if let Some(drag) = self.slider_drag {
                        self.update_slider_drag(drag.button);
                    }
                    if self.hovered().is_some() {
                        self.keyboard_selection = None;
                    }
                    let after = (self.hovered(), self.hovered_message_button());
                    self.frame_dirty |= (self.wait == Wait::Menu
                        && (before.0 != after.0 || self.dragged_button.is_some()))
                        || before.1 != after.1;
                    if self.slider_drag.is_none() && self.dragged_button.is_none() {
                        let button = if self.wait == Wait::Menu && before.0 != after.0 {
                            after.0.and_then(|index| self.menu_buttons()?.get(index))
                        } else if before.1 != after.1 {
                            after.1.and_then(|index| self.message_buttons()?.get(index))
                        } else {
                            None
                        };
                        if let Some(index) = button.map(|button| button.sounds[1]) {
                            self.play_button_sound(index)?;
                        }
                    }
                }
                RuntimeEvent::Wheel { delta_y, .. } if self.backlog_offset.is_some() => {
                    self.scroll_backlog(*delta_y < 0.0);
                }
                RuntimeEvent::Wheel { delta_y, .. } if self.wait == Wait::Menu => {
                    if *delta_y != 0.0
                        && self
                            .scroll_list
                            .as_ref()
                            .is_some_and(|list| !list.items.is_empty())
                    {
                        let offset = self.variable(99, 246)? + if *delta_y < 0.0 { 1 } else { -1 };
                        self.refresh_scroll_list(offset)?;
                    }
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                } if self.wait == Wait::Menu => {
                    if let Some(index) = self.hovered() {
                        if self
                            .menu_buttons()
                            .is_some_and(|buttons| buttons[index].drag.source)
                        {
                            self.dragged_button = Some(index);
                            self.frame_dirty = true;
                        } else {
                            self.update_slider_drag(index);
                        }
                    }
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Released,
                } => {
                    if let Some(drag) = self.slider_drag.take() {
                        self.frame_dirty = true;
                        if self.wait == Wait::Menu
                            && let Some((slider, value)) = self.slider_at(drag.button)
                        {
                            self.set_variable(slider.binding.0, slider.binding.1, value)?;
                            self.activate_menu_button(drag.button, drag.button)?;
                        }
                    } else if let Some(source) = self.dragged_button.take() {
                        self.frame_dirty = true;
                        if self.wait == Wait::Menu
                            && let Some((target, _)) = self.hovered_drop_target()
                        {
                            self.activate_menu_button(source, target)?;
                        }
                    } else if !self.click_backlog()?
                        && !self.click_message_button()?
                        && !self.click_scrollbar()?
                        && !self.invoke_input_route(2)?
                    {
                        self.advance(false)?;
                    }
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Secondary,
                    state: KeyState::Pressed,
                } if self.wait == Wait::Input
                    && !self.hidden_message
                    && self.message.is_some()
                    && self.script_menu.is_none()
                    && self.variable(99, 201)? != 0 =>
                {
                    self.mouse_gesture = Some(MouseGesture::default());
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Secondary,
                    state: KeyState::Released,
                } => {
                    if let Some(origin) =
                        self.mouse_gesture.take().and_then(|gesture| gesture.origin)
                    {
                        self.finish_mouse_gesture(origin)?;
                    } else if !self.invoke_input_route(3)? {
                        self.secondary_click()?;
                    }
                }
                RuntimeEvent::Key {
                    code: KeyCode::Enter | KeyCode::Space,
                    state: KeyState::Pressed,
                    ..
                } => {
                    if !self.invoke_input_route(2)? {
                        self.advance(true)?;
                    }
                }
                #[cfg(feature = "video-ffmpeg")]
                RuntimeEvent::Key {
                    code: KeyCode::Escape,
                    state: KeyState::Pressed,
                    ..
                } if self.wait == Wait::Movie => self.stop_movie()?,
                RuntimeEvent::Key {
                    code: KeyCode::Escape,
                    state: KeyState::Pressed,
                    ..
                } => {
                    if !self.invoke_input_route(3)? {
                        self.toggle_story_menu()?;
                    }
                }
                RuntimeEvent::Key {
                    code: KeyCode::ArrowUp | KeyCode::ArrowDown,
                    state: KeyState::Pressed,
                    ..
                } if self.backlog_offset.is_some() => {
                    self.scroll_backlog(matches!(
                        event,
                        RuntimeEvent::Key {
                            code: KeyCode::ArrowDown,
                            ..
                        }
                    ));
                }
                RuntimeEvent::Key {
                    code: KeyCode::ArrowUp | KeyCode::ArrowLeft,
                    state: KeyState::Pressed,
                    ..
                } if self.wait == Wait::Menu => self.move_menu_selection(false),
                RuntimeEvent::Key {
                    code: KeyCode::ArrowDown | KeyCode::ArrowRight | KeyCode::Tab,
                    state: KeyState::Pressed,
                    ..
                } if self.wait == Wait::Menu => self.move_menu_selection(true),
                _ => {}
            }
        }
        if (self.control_keys != 0 || self.skip_dialogue)
            && self.wait == Wait::Input
            && self.message.is_some()
        {
            if let Some(message) = &mut self.message {
                message.reveal();
            }
            if self.audio.as_ref().is_some_and(|audio| audio.playing(1)) {
                self.stop_sound(1)?;
            }
            self.advance(true)?;
        }
        if self.auto_dialogue
            && self.wait == Wait::Input
            && let Some(message) = &self.message
            && message.finished()
        {
            let mode = self.variable(99, 28)?;
            let voice_pending =
                mode == 1 && self.audio.as_ref().is_some_and(|audio| audio.playing(1));
            let delay = if mode == 2 && !self.script.dialect.classic() {
                Duration::from_secs(self.variable(99, 184)?.max(0) as u64)
            } else {
                let speed = self.variable(99, 29)?.clamp(0, 22) as u64;
                let bytes = self.text_encoding.encode(message.parts().0).0.len() as u64;
                Duration::from_millis(5 * (23 - speed) * (bytes / 2))
            };
            if message.elapsed() >= delay && !voice_pending {
                self.advance(true)?;
            }
        }
        self.run_message_effects()?;
        if let Wait::Timer { remaining, .. } = &mut self.wait {
            *remaining = remaining.saturating_sub(elapsed);
            if remaining.is_zero() {
                self.wait = Wait::Ready;
            }
        }
        if let Wait::VoiceTail(remaining) = &mut self.wait {
            if self.audio.as_ref().is_some_and(|audio| audio.playing(1)) {
                *remaining = Duration::from_secs(1);
            } else {
                *remaining = remaining.saturating_sub(elapsed);
                if remaining.is_zero() {
                    self.wait = Wait::Ready;
                }
            }
        }
        if let Wait::Audio { flags, .. } = self.wait
            && flags & 4 != 0
            && (self.control_keys != 0 || self.skip_dialogue)
        {
            for channel in 2..=5 {
                if self
                    .audio
                    .as_ref()
                    .is_some_and(|audio| audio.playing(channel))
                {
                    self.stop_sound(channel)?;
                }
            }
        }
        if let Wait::Audio {
            flags,
            start_timeout,
        } = &mut self.wait
        {
            *start_timeout = start_timeout.saturating_sub(elapsed);
            let playing = |channel| {
                self.audio
                    .as_ref()
                    .is_some_and(|audio| audio.playing(channel))
            };
            let waiting = (*flags & 1 != 0 && playing(0))
                || (*flags & 2 != 0 && playing(1))
                || (*flags & 4 != 0 && (2..=5).any(playing))
                || (*flags & 32 != 0 && !playing(0));
            if !waiting || (*flags == 32 && start_timeout.is_zero()) {
                self.wait = Wait::Ready;
            }
        }
        if self.wait == Wait::Motion
            && self.fade.is_none()
            && self.pan.is_none()
            && self.scene_shake.is_none()
            && self.camera_motion.is_none()
            && self.scene_translation_motion.is_none()
            && !self
                .panning_planes
                .values()
                .any(|plane| plane.motions.iter().any(Option::is_some))
            && self.transition.is_none()
            && self.screen_break.is_none()
            && !self.cut_in.active()
            && !self.actors.values().any(|actor| {
                actor.layer.active()
                    || actor.motion.is_some()
                    || actor.wave.is_some()
                    || actor.shake.is_some()
            })
            && self.mosaic_tween.is_none()
            && !self.animation.as_ref().is_some_and(Animation::busy)
        {
            self.wait = Wait::Ready;
        }
        if self.wait != Wait::Finished && self.music_end_route.is_some() {
            if self.audio.as_ref().is_some_and(|audio| audio.playing(0)) {
                self.music_idle = Duration::ZERO;
            } else {
                self.music_idle = self.music_idle.saturating_add(elapsed);
                if self.music_idle >= Duration::from_millis(500)
                    && let Some(label) = self.music_end_route.take()
                {
                    self.jump(&label)?;
                    self.wait = Wait::Ready;
                }
            }
        }
        if self.wait == Wait::Menu
            && self.slider_drag.is_none()
            && self
                .menu_buttons()
                .is_some_and(|buttons| buttons.iter().any(|button| button.music_slider))
            && let Some(position) = self.audio.as_ref().and_then(Audio::music_position)
            && self.variable(99, 189)? != i32::from(position)
        {
            self.set_variable(99, 189, i32::from(position))?;
            self.frame_dirty = true;
        }
        if self.wait == Wait::Ready {
            self.message_macros.fill(None);
            self.message_macro_slot = 0;
            let mut executed = 0;
            while self.wait == Wait::Ready && executed < TICK_BUDGET {
                let offset = self.ip;
                if let Err(source) = self.step() {
                    return Err(match source {
                        Error::UnsupportedCommand { .. } => source,
                        source => Error::ScriptExecution {
                            script: self.script_name.clone(),
                            offset,
                            source: Box::new(source),
                        },
                    });
                }
                executed += 1;
            }
            if executed == TICK_BUDGET && self.wait == Wait::Ready {
                return Err(Error::Invalid("ADV32 instruction cycle without a wait"));
            }
            if self.wait != Wait::Ready {
                self.actor_anchor = None;
            }
        }
        self.run_message_effects()?;
        Ok(())
    }

    fn invoke_input_route(&mut self, input: u8) -> Result<bool> {
        if !matches!(self.wait, Wait::Input | Wait::Timer { .. } | Wait::Motion) {
            return Ok(false);
        }
        let Some(label) = self
            .button_routes
            .get(&input)
            .filter(|label| !label.is_empty() && label.as_slice() != b"-")
            .cloned()
        else {
            return Ok(false);
        };
        self.jump(&label)?;
        self.wait = Wait::Ready;
        self.frame_dirty = true;
        Ok(true)
    }

    fn advance(&mut self, keyboard: bool) -> Result<()> {
        if self.backlog_offset.is_some()
            && self.wait == Wait::Input
            && self.return_from_script_menu()?
        {
            return Ok(());
        }
        match self.wait {
            #[cfg(feature = "video-ffmpeg")]
            Wait::Movie => self.stop_movie()?,
            Wait::Input
            | Wait::Timer {
                skippable: true, ..
            } => {
                if self.hidden_message {
                    self.hidden_message = false;
                    self.frame_dirty = true;
                    return Ok(());
                }
                if self.wait == Wait::Input
                    && let Some(message) = &mut self.message
                    && !message.finished()
                {
                    message.reveal();
                    self.frame_dirty = true;
                    return Ok(());
                }
                if let Some(pan) = self.pan.take() {
                    self.background_position = pan.to;
                    self.frame_dirty = true;
                }
                self.wait = Wait::Ready;
            }
            Wait::Menu => {
                let hovered = self.hovered().filter(|&index| {
                    self.menu_buttons()
                        .is_some_and(|buttons| !buttons[index].drag.source)
                });
                let selected = if keyboard {
                    self.keyboard_selection.or_else(|| {
                        let buttons = self.menu_buttons()?;
                        hovered
                            .filter(|&index| self.menu_selectable(&buttons[index]))
                            .or_else(|| {
                                buttons.iter().position(|button| self.menu_selectable(button))
                            })
                    })
                } else {
                    hovered
                };
                if let Some(index) = selected {
                    self.activate_menu_button(index, index)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn activate_menu_button(&mut self, index: usize, selected: usize) -> Result<()> {
        let Some(button) = self.menu_buttons().and_then(|buttons| buttons.get(index)) else {
            return Ok(());
        };
        if button.label.is_empty() {
            return Ok(());
        }
        let label = button.label.clone();
        let (slot, list_index, area) = (button.save_slot, button.list_index, button.area);
        let select_sound = button.sounds[0];
        let slider = button.slider.filter(|_| button.music_slider);
        self.set_variable(99, 245, selected as i32)?;
        if let Some(slider) = slider {
            let (bank, index) = slider.binding;
            let position = self.variable(bank, index)?;
            self.set_variable(99, 187, position)?;
        }
        if let Some(slot) = slot {
            self.set_variable(99, 254, i32::from(slot))?;
        }
        if let Some(index) = list_index {
            self.set_variable(99, 254, i32::from(index))?;
            let row = self
                .slot_rectangles
                .iter()
                .position(|rect| *rect == area)
                .unwrap_or(0);
            self.set_variable(99, 253, row as i32)?;
        }
        self.play_button_sound(select_sound)?;
        if label[0] == b':' {
            return self.menu_action(&label);
        }
        self.jump(&label)?;
        self.choices = None;
        self.slider_drag = None;
        self.dragged_button = None;
        self.confirmation = None;
        self.menu_variant = None;
        self.wait = Wait::Ready;
        self.frame_dirty = true;
        self.active_menu_origin = (0, 0);
        self.menu_pan = None;
        self.keyboard_selection = None;
        Ok(())
    }

    fn menu_action(&mut self, action: &[u8]) -> Result<()> {
        if self.quick_prompt.is_some() && matches!(action, b":y" | b":n") {
            return self.finish_quick_prompt(action == b":y");
        }
        if let Some(confirmation) = self.confirmation {
            match (action, confirmation) {
                (b":y" | b":n", _) => {
                    let selected = u8::from(action == b":n");
                    if let Some(label) = self.button_routes.get(&selected).cloned() {
                        self.jump(&label)?;
                    }
                    self.confirmation = None;
                    self.menu_variant = None;
                    self.wait = Wait::Ready;
                    self.frame_dirty = true;
                    self.active_menu_origin = (0, 0);
                    self.keyboard_selection = None;
                    return Ok(());
                }
                (b":c", Confirmation::Remember(preference)) => {
                    let selected = self.variable(93, preference)?;
                    self.set_variable(93, preference, i32::from(selected == 0))?;
                    let paired = match preference {
                        1 => Some(11),
                        11 => Some(1),
                        2 => Some(3),
                        3 => Some(2),
                        9 => Some(10),
                        10 => Some(9),
                        _ => None,
                    };
                    if let Some(paired) = paired {
                        let selected = self.variable(93, paired)?;
                        self.set_variable(93, paired, i32::from(selected == 0))?;
                    }
                    self.frame_dirty = true;
                    return Ok(());
                }
                _ => {}
            }
        }
        match action {
            b":n" => self.hidden_message = !self.hidden_message,
            b":c" | b":clear" => {
                if !self.return_from_script_menu()? {
                    self.wait = Wait::Ready;
                }
            }
            b":ds" | b":aasave" => self.open_script_menu(2)?,
            b":dl" | b":aaload" => self.open_script_menu(3)?,
            b":option" | b":aaconfig" => self.open_script_menu(5)?,
            b":backlog" | b":aalog" => self.open_script_menu(6)?,
            b":manual" => {
                if self.game.resource("mes", "MANUAL", "mes").is_ok() {
                    self.open_named_menu(b"MANUAL", false)?;
                }
            }
            b":qs" | b":aaqsave" => self.request_quick_operation(QuickOperation::Save)?,
            b":ql" | b":aaqload" => self.request_quick_operation(QuickOperation::Load)?,
            b":rapid" | b":r" | b":aaskip" => {
                self.auto_dialogue = false;
                self.skip_dialogue = !self.skip_dialogue;
                self.return_from_script_menu()?;
            }
            b":auto" => {
                self.skip_dialogue = false;
                self.auto_dialogue = !self.auto_dialogue;
                self.return_from_script_menu()?;
            }
            b":t" | b":voice" => {
                if let Some(voice) = self.last_voice.clone() {
                    self.play_sound(&voice, 1, false)?;
                }
            }
            b":aawin_size" => {
                self.display_preference ^= 1;
                self.set_fullscreen(self.display_preference != 0);
            }
            b":aatitle" => {
                if let Some(title) = self
                    .sound_names
                    .system(1)
                    .map(|name| name.as_bytes().to_vec())
                {
                    self.script_menu = None;
                    self.switch_script(&title)?;
                    self.wait = Wait::Ready;
                }
            }
            _ => return Err(Error::Unsupported("ADV32 native menu action")),
        }
        self.frame_dirty = true;
        Ok(())
    }

    fn menu_selectable(&self, button: &Button) -> bool {
        button.selectable() || (self.confirmation.is_some() && button.label == b":n")
    }

    fn menu_buttons(&self) -> Option<&[Button]> {
        self.choices
            .as_deref()
            .or_else(|| self.parties.get(&self.button_group).map(Vec::as_slice))
    }

    fn add_classic_route_choices(&mut self, next_page: bool) -> Result<()> {
        const ROUTES: [&[u8]; 5] = [b"label01", b"label02", b"label03", b"label04", b"label05"];
        const END: &[u8] = b"label06";
        const NEXT: &[u8] = b"xtsel";
        const BACK: &[u8] = b"startsel";
        let mut eligible = [false; 5];
        for (index, enabled) in eligible.iter_mut().enumerate() {
            *enabled = self.variable(5, index as u16)? >= 2;
        }
        let count = eligible.iter().filter(|&&enabled| enabled).count();
        let mut labels = Vec::new();
        if count == 0 {
            labels.push(END);
        } else if next_page {
            labels.extend(
                (3..count)
                    .filter(|&index| eligible[index])
                    .map(|index| ROUTES[index]),
            );
            labels.push(BACK);
        } else {
            labels.extend(
                ROUTES
                    .into_iter()
                    .zip(eligible)
                    .filter_map(|(label, enabled)| enabled.then_some(label))
                    .take(3),
            );
            if count > 3 {
                labels.push(NEXT);
            }
        }
        if labels
            .iter()
            .any(|label| !self.classic_choice_text.contains_key(*label))
        {
            let root = self
                .game
                .data
                .parent()
                .ok_or(Error::Invalid("ADV32 game directory"))?;
            if let Some(captions) = choice_captions(root, &labels, self.text_encoding)? {
                for (label, text) in captions {
                    self.classic_choice_text.entry(label).or_insert(text);
                }
            }
        }
        for label in labels {
            let caption = self
                .classic_choice_text
                .get(label)
                .cloned()
                .ok_or(Error::Invalid("ADV32 route choice caption"))?;
            self.add_choice(label, caption);
        }
        Ok(())
    }

    fn add_choice(&mut self, label: &[u8], caption: String) {
        let Some(choices) = &mut self.choices else {
            return;
        };
        let index = choices.iter().filter(|button| button.selectable()).count();
        let template = self
            .parties
            .get(&3)
            .into_iter()
            .flatten()
            .filter(|button| button.selectable())
            .nth(index);
        let mut button = template.cloned().unwrap_or_else(|| {
            let (width, height) = (self.extent.0 as i32, self.extent.1 as i32);
            let top = height / 12 + index as i32 * (height / 8);
            Button {
                area: [width / 4, top, width * 3 / 4, top + height / 10],
                ..Button::default()
            }
        });
        button.label = label.to_vec();
        if button.captions.is_empty() {
            let [left, top, right, bottom] = button.area;
            button.captions.insert(
                99,
                vec![Text {
                    fixed_pitch: false,
                    line_height: None,
                    outline: false,
                    clip_width: None,
                    content: caption,
                    x: left + 24,
                    y: (top + bottom) / 2 - 14,
                    width: Some((right - left - 48).max(1) as u32),
                    size: 26,
                    color: [255; 3],
                }],
            );
        } else {
            for text in button.captions.values_mut().flatten() {
                text.content.clone_from(&caption);
            }
        }
        choices.push(button);
    }

    fn advance_mouse_gesture(&mut self, elapsed: Duration) -> Result<()> {
        let Some(mut gesture) = self.mouse_gesture.take() else {
            return Ok(());
        };
        if self.wait != Wait::Input || self.message.is_none() || self.hidden_message {
            self.frame_dirty |= gesture.origin.is_some();
            return Ok(());
        }
        if gesture.origin.is_none() {
            gesture.held = gesture.held.saturating_add(elapsed);
            let delay = if self.script.dialect.version == 0x9e97 {
                Duration::from_secs(1)
            } else {
                Duration::from_millis(300 * self.variable(99, 194)?.max(0) as u64)
            };
            if gesture.held > delay {
                gesture.origin = Some(self.pointer);
                self.frame_dirty = true;
                if !self.script.dialect.classic() {
                    self.skip_dialogue = false;
                    self.auto_dialogue = false;
                }
            }
        }
        self.mouse_gesture = Some(gesture);
        Ok(())
    }

    fn finish_mouse_gesture(&mut self, origin: (i32, i32)) -> Result<()> {
        self.frame_dirty = true;
        let dx = self.pointer.0 - origin.0;
        let dy = self.pointer.1 - origin.1;
        let binding = if dy > 64 && dx.abs() < 100 {
            197
        } else if dx < -64 && dy.abs() < 100 {
            196
        } else if dy < -64 && dx.abs() < 100 {
            199
        } else if dx > 64 && dy.abs() < 100 {
            198
        } else {
            return Ok(());
        };
        let action = self.variable(99, binding)?;
        let script_menu_available = self.variable(9, 7)? == 0;
        match action {
            0 | 1 => {
                self.set_variable(99, 30, i32::from(action == 0))?;
                self.refresh_message_menu()?;
                self.menu_action(b":rapid")?;
            }
            2 => self.menu_action(b":auto")?,
            3 => self.open_script_menu(6)?,
            4 if script_menu_available => self.open_script_menu(2)?,
            5 if script_menu_available => self.open_script_menu(3)?,
            6 => self.open_script_menu(5)?,
            7 if script_menu_available => self.request_quick_operation(QuickOperation::Save)?,
            8 if script_menu_available => self.request_quick_operation(QuickOperation::Load)?,
            9 => self.menu_action(b":aawin_size")?,
            10 => self.menu_action(b":manual")?,
            11 => self.menu_action(b":aatitle")?,
            _ => {}
        }
        Ok(())
    }

    fn secondary_click(&mut self) -> Result<()> {
        if self.wait != Wait::Input || self.message.is_none() || self.script_menu.is_some() {
            return self.toggle_story_menu();
        }
        if self.hidden_message || self.skip_dialogue || self.auto_dialogue {
            self.hidden_message = false;
            self.skip_dialogue = false;
            self.auto_dialogue = false;
            self.frame_dirty = true;
            return Ok(());
        }
        if self.variable(9, 7)? != 0 {
            return self.open_script_menu(4);
        }

        let preference = self.variable(99, 211)?;
        let mode = if self.script.dialect.classic() {
            [1, 2, 0].get(preference as usize).copied().unwrap_or(0)
        } else {
            preference
        };
        match mode {
            1 => {
                self.hidden_message = true;
                self.frame_dirty = true;
                Ok(())
            }
            2 => self.open_script_menu(6),
            _ => self.open_script_menu(4),
        }
    }

    fn request_quick_operation(&mut self, operation: QuickOperation) -> Result<()> {
        if matches!(operation, QuickOperation::Load) && !self.saves.exists(2) {
            return Ok(());
        }
        if self.wait != Wait::Input || self.script_menu.is_some() {
            return self.perform_quick_operation(operation);
        }
        let (preference, origin) = match (self.script.dialect.classic(), operation) {
            (true, QuickOperation::Save) => (5, (292, 160)),
            (true, QuickOperation::Load) => (4, (292, 160)),
            (false, QuickOperation::Save) => (7, (422, 248)),
            (false, QuickOperation::Load) => (8, (422, 248)),
        };
        if self.variable(93, preference)? != 0 {
            return self.perform_quick_operation(operation);
        }
        if self
            .parties
            .get(&2)
            .is_some_and(|buttons| buttons.iter().any(|button| button.label == b":y"))
        {
            let save = if matches!(operation, QuickOperation::Save) {
                Some((self.snapshot()?, self.compose()?))
            } else {
                None
            };
            self.quick_prompt = Some(QuickPrompt {
                operation,
                save,
                button_group: self.button_group,
                origin: self.active_menu_origin,
            });
            self.confirmation = Some(Confirmation::Remember(preference));
            self.menu_variant = Some(preference);
            self.button_group = 2;
            self.active_menu_origin = origin;
            self.keyboard_selection = None;
            self.wait = Wait::Menu;
            self.frame_dirty = true;
        }
        Ok(())
    }

    fn finish_quick_prompt(&mut self, accepted: bool) -> Result<()> {
        if let Some(prompt) = self.quick_prompt.take() {
            self.button_group = prompt.button_group;
            self.active_menu_origin = prompt.origin;
            self.confirmation = None;
            self.menu_variant = None;
            self.keyboard_selection = None;
            self.wait = Wait::Input;
            self.frame_dirty = true;
            if accepted {
                if let Some((story, thumbnail)) = prompt.save {
                    self.saves.write_slot(2, &story, &thumbnail)?;
                    self.refresh_message_menu()?;
                } else {
                    self.perform_quick_operation(prompt.operation)?;
                }
            }
        }
        Ok(())
    }

    fn perform_quick_operation(&mut self, operation: QuickOperation) -> Result<()> {
        match operation {
            QuickOperation::Save => {
                self.save_slot(2)?;
                self.refresh_message_menu()
            }
            QuickOperation::Load => self.load_slot(2),
        }
    }

    fn refresh_message_menu(&mut self) -> Result<()> {
        let script = match self.game.script("MESMENU") {
            Ok(script) => Arc::new(script),
            Err(Error::MissingAsset(_)) => return Ok(()),
            Err(error) => return Err(error),
        };
        let entry = script.entry(0)?;
        let program = (
            std::mem::replace(&mut self.script, script),
            std::mem::replace(&mut self.script_name, "MESMENU".into()),
            std::mem::replace(&mut self.ip, entry),
            std::mem::replace(&mut self.wait, Wait::Ready),
        );
        let mut parties = std::mem::take(&mut self.parties);
        let mut templates = std::mem::take(&mut self.templates);
        let mut message_buttons = std::mem::take(&mut self.message_button_banks);
        let overlays = std::mem::take(&mut self.overlays);
        let captions = std::mem::take(&mut self.overlay_text);
        let variables = self.variables.clone();
        let tick = (self.tick_label, self.tick_step);
        let group = std::mem::take(&mut self.button_group);
        let blit_offset = std::mem::take(&mut self.blit_offset);
        let style = (self.button_options, self.button_highlight);

        let result = (|| {
            for _ in 0..TICK_BUDGET {
                let instruction = self.script.instruction(self.ip)?;
                let code = self.script.dialect.opcode(instruction.opcode);
                if matches!(code, Some(0x00 | 0x1d)) {
                    return Ok(());
                }
                self.step()?;
                if self.wait != Wait::Ready {
                    break;
                }
            }
            Err(Error::Invalid("ADV32 message-menu setup did not return"))
        })();
        if result.is_ok() {
            self.parties.remove(&0);
            parties.append(&mut self.parties);
            templates.append(&mut self.templates);
            message_buttons.append(&mut self.message_button_banks);
            self.frame_dirty = true;
        }
        (self.script, self.script_name, self.ip, self.wait) = program;
        (self.tick_label, self.tick_step) = tick;
        (self.button_options, self.button_highlight) = style;
        self.button_group = group;
        self.blit_offset = blit_offset;
        self.parties = parties;
        self.templates = templates;
        self.message_button_banks = message_buttons;
        self.overlays = overlays;
        self.overlay_text = captions;
        self.variables = variables;
        result
    }

    fn toggle_story_menu(&mut self) -> Result<()> {
        if self.quick_prompt.is_some() {
            return self.finish_quick_prompt(false);
        }
        if self.wait == Wait::Menu && self.confirmation.is_some() {
            let can_cancel = self
                .menu_buttons()
                .is_some_and(|buttons| buttons.iter().any(|button| button.label == b":n"));
            return self.menu_action(if can_cancel { b":n" } else { b":y" });
        }
        if self.return_from_script_menu()? {
            return Ok(());
        }
        match self.wait {
            Wait::Input if self.message.is_some() => self.open_script_menu(4)?,
            Wait::Menu if self.choices.is_some() => self.open_script_menu(4)?,
            _ => {}
        }
        Ok(())
    }

    fn open_script_menu(&mut self, system: usize) -> Result<()> {
        let name = if system == 5 && self.game.resource("mes", "CONFIG", "mes").is_ok() {
            Some("CONFIG")
        } else {
            self.sound_names.system(system)
        };
        let Some(name) = name.map(|name| name.as_bytes().to_vec()) else {
            return Ok(());
        };
        self.open_named_menu(&name, system == 6)
    }

    fn open_named_menu(&mut self, name: &[u8], backlog: bool) -> Result<()> {
        if self.script_menu.is_none() {
            self.script_menu = Some(ScriptMenu {
                story: self.snapshot()?,
                thumbnail: self.compose()?,
                return_wait: self.wait,
                buttons: self.parties.clone(),
                group: self.button_group,
                choices: self.choices.take(),
            });
        }
        self.switch_script(name)?;
        self.backlog_offset = backlog.then_some(0);
        self.slot_rectangles.clear();
        self.slot_thumbnails.clear();
        self.button_group = 0;
        self.keyboard_selection = None;
        self.active_menu_origin = (0, 0);
        self.wait = Wait::Ready;
        self.frame_dirty = true;
        Ok(())
    }

    fn return_from_script_menu(&mut self) -> Result<bool> {
        let Some(menu) = self.script_menu.take() else {
            return Ok(false);
        };
        let instruction = menu.story.instruction;
        self.restore_snapshot(menu.story, instruction)?;
        self.parties = menu.buttons;
        self.choices = menu.choices;
        self.backlog_offset = None;
        self.button_group = menu.group;
        self.confirmation = None;
        self.menu_variant = None;
        self.keyboard_selection = None;
        self.active_menu_origin = (0, 0);
        self.wait = menu.return_wait;
        self.refresh_message_menu()?;
        Ok(true)
    }

    fn click_backlog(&mut self) -> Result<bool> {
        let Some(offset) = self.backlog_offset else {
            return Ok(false);
        };
        if self.wait != Wait::Input {
            return Ok(false);
        }
        let classic = self.script.dialect.classic();
        let layout = LogLayout::for_dialect(classic);
        if LogLayout::contains(layout.back, self.pointer) {
            self.return_from_script_menu()?;
        } else if LogLayout::contains(layout.scrollbar, self.pointer) {
            let [_, top, _, bottom] = layout.scrollbar;
            let pages = self.backlog.len().saturating_sub(layout.rows);
            let position = (self.pointer.1 - top) as f64 / (bottom - top) as f64;
            self.backlog_offset = Some(pages - (position * pages as f64).round() as usize);
            self.frame_dirty = true;
        } else {
            let count = self.backlog.len().saturating_sub(offset).min(layout.rows);
            let oldest = self.backlog.len().saturating_sub(offset + count);
            let entry = (0..count)
                .find(|&row| LogLayout::contains(layout.jump_area(classic, row), self.pointer));
            if let Some(index) = entry.map(|row| oldest + row)
                && let Some(checkpoint) = self
                    .backlog
                    .get(index)
                    .and_then(|entry| entry.checkpoint.as_deref())
                    .cloned()
            {
                let instruction = checkpoint.instruction;
                self.script_menu = None;
                self.backlog_offset = None;
                self.auto_dialogue = false;
                self.skip_dialogue = false;
                self.restore_snapshot(checkpoint, instruction)?;
                self.last_voice = self
                    .backlog
                    .get(index)
                    .and_then(|entry| entry.voice.clone());
                self.backlog.truncate(index + 1);
                self.wait = Wait::Input;
                self.refresh_message_menu()?;
                return Ok(true);
            }
            let voice = (0..count)
                .find(|&row| LogLayout::contains(layout.voice_area(classic, row), self.pointer))
                .and_then(|row| self.backlog.get(oldest + row))
                .and_then(|entry| entry.voice.clone());
            if let Some(voice) = voice {
                self.play_sound(&voice, 1, false)?;
            }
        }
        Ok(true)
    }

    fn scroll_backlog(&mut self, towards_newer: bool) {
        if let Some(offset) = &mut self.backlog_offset {
            *offset = if towards_newer {
                offset.saturating_sub(1)
            } else {
                offset.saturating_add(1).min(
                    self.backlog
                        .len()
                        .saturating_sub(LogLayout::for_dialect(self.script.dialect.classic()).rows),
                )
            };
            self.frame_dirty = true;
        }
    }

    fn log_message(
        &mut self,
        line: String,
        speaker: String,
        voice: Option<String>,
        checkpoint: Option<Box<NativeSlot>>,
    ) {
        while self.backlog.len() >= 2048 {
            self.backlog.pop_front();
        }
        self.backlog.push_back(BacklogEntry {
            speaker,
            line,
            voice,
            checkpoint,
        });
    }

    fn record_current_dialogue(&mut self) -> Result<()> {
        if !std::mem::take(&mut self.pending_log_entry) {
            return Ok(());
        }
        if let Some(message) = &self.message {
            let (line, speaker) = message.parts();
            let (line, speaker) = (line.to_owned(), speaker.to_owned());
            let voice = self.pending_voice.take();
            let checkpoint = if self.wait == Wait::Input {
                Some(Box::new(self.snapshot()?))
            } else {
                None
            };
            self.log_message(line, speaker, voice, checkpoint);
        }
        Ok(())
    }

    fn move_menu_selection(&mut self, forward: bool) {
        let Some(buttons) = self.menu_buttons() else {
            return;
        };
        let selected = self.keyboard_selection.or_else(|| self.hovered());
        let candidates = buttons
            .iter()
            .enumerate()
            .filter(|(_, button)| self.menu_selectable(button));
        let next = if forward {
            let mut choices = candidates.map(|(index, _)| index);
            let first = choices.next();
            match selected {
                Some(selected) => first
                    .filter(|index| *index > selected)
                    .or_else(|| choices.find(|index| *index > selected))
                    .or(first),
                None => first,
            }
        } else {
            let mut choices = candidates.rev().map(|(index, _)| index);
            let last = choices.next();
            match selected {
                Some(selected) => last
                    .filter(|index| *index < selected)
                    .or_else(|| choices.find(|index| *index < selected))
                    .or(last),
                None => last,
            }
        };
        self.keyboard_selection = next;
        self.frame_dirty = true;
    }

    fn message_buttons_visible(&self) -> bool {
        self.wait == Wait::Input && !self.hidden_message && self.message.is_some()
    }

    fn message_buttons(&self) -> Option<&[Button]> {
        self.message_buttons_visible()
            .then(|| self.message_button_banks.get(&self.active_message_bank))
            .flatten()
            .map(|bank| bank.buttons.as_slice())
    }

    fn hovered_message_button(&self) -> Option<usize> {
        self.hit_button(self.message_buttons()?, (0, 0))
    }

    fn click_message_button(&mut self) -> Result<bool> {
        let Some((action, select_sound)) = self
            .hovered_message_button()
            .and_then(|index| self.message_buttons()?.get(index))
            .filter(|button| button.label.starts_with(b":"))
            .map(|button| (button.label.clone(), button.sounds[0]))
        else {
            return Ok(false);
        };
        self.play_button_sound(select_sound)?;
        if action == b":n" {
            let next_bank = if self.active_message_bank == EXPANDED_MESSAGE_BANK {
                COMPACT_MESSAGE_BANK
            } else {
                EXPANDED_MESSAGE_BANK
            };
            if self.message_button_banks.contains_key(&next_bank) {
                self.active_message_bank = next_bank;
                self.frame_dirty = true;
            }
        } else {
            self.menu_action(&action)?;
        }
        Ok(true)
    }

    fn hovered(&self) -> Option<usize> {
        self.hit_button(self.menu_buttons()?, self.active_menu_origin)
    }

    fn hovered_drop_target(&self) -> Option<(usize, [i32; 4])> {
        let (x, y) = (
            self.pointer.0 - self.active_menu_origin.0,
            self.pointer.1 - self.active_menu_origin.1,
        );
        self.menu_buttons()?
            .iter()
            .filter(|button| button.drag.target)
            .map(|button| button.area)
            .enumerate()
            .find(|&(_, [left, top, right, bottom])| left < x && x < right && top < y && y < bottom)
    }

    fn hit_button(&self, buttons: &[Button], origin: (i32, i32)) -> Option<usize> {
        let (x, y) = (self.pointer.0 - origin.0, self.pointer.1 - origin.1);
        buttons.iter().rposition(|button| button.hit_at(x, y))
    }

    fn set_button_sounds(&mut self, sounds: [u16; 2]) {
        if let Some(button) = self
            .parties
            .get_mut(&self.button_group)
            .and_then(|buttons| buttons.last_mut())
        {
            for (stored, sound) in button.sounds.iter_mut().zip(sounds) {
                if sound != 255 {
                    *stored = sound;
                }
            }
        }
    }

    fn slider_at(&self, index: usize) -> Option<(Slider, i32)> {
        let button = self.menu_buttons()?.get(index)?;
        let slider = button.slider?;
        let half_thumb = button
            .states
            .get(&6)
            .and_then(|images| images.first())
            .and_then(|sprite| sprite.clip)
            .map_or(0, |clip| (clip.width / 2) as i32);
        let value = slider.value_at(self.pointer.0 - self.active_menu_origin.0 - half_thumb);
        Some((slider, value))
    }

    fn update_slider_drag(&mut self, index: usize) {
        if let Some((_, value)) = self.slider_at(index) {
            let drag = Some(SliderDrag {
                button: index,
                value,
            });
            self.frame_dirty |= self.slider_drag != drag;
            self.slider_drag = drag;
        }
    }

    fn click_scrollbar(&mut self) -> Result<bool> {
        let Some(list) = &self.scroll_list else {
            return Ok(false);
        };
        let available = list.items.len().saturating_sub(self.slot_rectangles.len());
        let [left, top, right, bottom] = list.bounds;
        let (x, y) = self.pointer;
        if self.wait != Wait::Menu
            || available == 0
            || !(left..right).contains(&x)
            || !(top..bottom).contains(&y)
        {
            return Ok(false);
        }
        let offset = (y - top) as f64 * available as f64 / (bottom - top) as f64;
        self.refresh_scroll_list(offset.round() as i32)?;
        Ok(true)
    }

    fn refresh_scroll_list(&mut self, offset: i32) -> Result<()> {
        let Some(list) = &self.scroll_list else {
            return Ok(());
        };
        if list.items.is_empty() || self.slot_rectangles.is_empty() {
            return Ok(());
        }
        let offset = (offset.max(0) as usize)
            .min(list.items.len().saturating_sub(self.slot_rectangles.len()));
        let buttons = self.parties.entry(self.button_group).or_default();
        buttons.retain(|button| button.list_index.is_none());
        for (item, &area) in list.items.iter().skip(offset).zip(&self.slot_rectangles) {
            let mut button = item.clone();
            button.area = area;
            for sprite in button.states.values_mut().flatten() {
                sprite.x += area[0];
                sprite.y += area[1];
            }
            for caption in button.captions.values_mut().flatten() {
                caption.x += area[0];
                caption.y += area[1];
            }
            if button.captions.is_empty()
                && let Some(title) = item
                    .list_index
                    .and_then(|index| self.music_titles.get(usize::from(index)))
            {
                button.captions.insert(
                    99,
                    vec![Text {
                        fixed_pitch: false,
                        line_height: None,
                        outline: false,
                        clip_width: None,
                        content: title.clone(),
                        x: area[0] + 16,
                        y: area[1] + 8,
                        size: 22,
                        color: [255; 3],
                        width: Some((area[2] - area[0]).saturating_sub(32).max(1) as u32),
                    }],
                );
            }
            buttons.push(button);
        }
        self.keyboard_selection = None;
        self.frame_dirty = true;
        self.set_variable(99, 246, offset as i32)
    }

    fn compose(&mut self) -> Result<RgbaImage> {
        match self.paint(false)? {
            Canvas::Raster(image) => Ok(image),
            #[cfg(not(target_env = "ohos"))]
            Canvas::Scene(_) => unreachable!("snapshot uses a raster canvas"),
        }
    }

    fn paint(&mut self, gpu: bool) -> Result<Canvas> {
        let (width, height) = self.extent;
        #[cfg(feature = "video-ffmpeg")]
        let gpu = gpu && self.movie.is_none();
        let mut frame = Canvas::new(width, height, gpu);
        if let Some(entry) = &self.name_entry {
            if let Some(background) = &entry.backdrop {
                graphics::blit(&mut frame, background, 0, 0, None);
            }
            let x = (width as i32 - 500) / 2;
            let y = (height as i32 - 220) / 2;
            let (family, given, confirm) = if self.text_encoding == encoding_rs::GBK {
                ("姓", "名", "Enter 确认 · Esc 取消")
            } else {
                ("姓", "名", "Enter 決定 · Esc キャンセル")
            };
            let painter = &mut self.text_painter;
            for (row, (label, name)) in [(family, &entry.names.family), (given, &entry.names.given)]
                .into_iter()
                .enumerate()
            {
                let top = y + (row as i32) * 60;
                let color = if entry.given_selected == (row == 1) {
                    [255, 229, 163]
                } else {
                    [255; 3]
                };
                for (text, offset) in [(label, 0), (name.as_str(), 100)] {
                    painter.draw(
                        &mut frame,
                        &Text {
                            fixed_pitch: false,
                            line_height: None,
                            outline: false,
                            clip_width: None,
                            content: text.to_owned(),
                            x: x + offset,
                            y: top,
                            size: 30,
                            color,
                            width: Some(400),
                        },
                    )?;
                }
            }
            painter.draw(
                &mut frame,
                &Text {
                    fixed_pitch: false,
                    line_height: None,
                    outline: false,
                    clip_width: None,
                    content: confirm.to_owned(),
                    x,
                    y: y + 138,
                    size: 20,
                    color: [255; 3],
                    width: Some(500),
                },
            )?;
            return Ok(frame);
        }
        if let Some(roll) = &self.staff_roll {
            roll.draw(&mut frame);
            return Ok(frame);
        }
        #[cfg(feature = "video-ffmpeg")]
        if let Some(movie) = &self.movie {
            if let Some(raster) = movie.frame() {
                let (video_width, video_height) = raster.dimensions();
                if (video_width, video_height) == self.extent {
                    return Ok(Canvas::Raster(raster.clone()));
                }
                let scale =
                    (width as f64 / video_width as f64).min(height as f64 / video_height as f64);
                let scaled_width = (video_width as f64 * scale).round() as u32;
                let scaled_height = (video_height as f64 * scale).round() as u32;
                let scaled = image::imageops::resize(
                    raster,
                    scaled_width,
                    scaled_height,
                    image::imageops::FilterType::Triangle,
                );
                image::imageops::overlay(
                    frame.raster_mut(),
                    &scaled,
                    i64::from((width - scaled_width) / 2),
                    i64::from((height - scaled_height) / 2),
                );
            }
            return Ok(frame);
        }
        let (shake_x, shake_y) = self.scene_shake.as_ref().map_or((0, 0), Shake::offset);
        let (shift_x, shift_y) = self.scene_translation;
        if let Some(background) = &self.background {
            let (mut x, mut y) = self.background_position;
            x += shake_x + shift_x;
            y += shake_y + shift_y;
            if self.centered_background {
                x += (width as i32 - background.image.width() as i32) / 2 - background.x;
                y += (height as i32 - background.image.height() as i32) / 2 - background.y;
            }
            let (camera_x, camera_y) = self.camera.center;
            if self.camera.scale == 100 && self.camera.center == (0, 0) {
                graphics::blit(&mut frame, background, x, y, None);
            } else {
                let scale = self.camera.scale as i32;
                x += (width as i32 / 2 - camera_x) * (100 - scale) / 100;
                y += (height as i32 / 2 - camera_y) * (100 - scale) / 100;
                graphics::blit_alpha(&mut frame, background, x, y, None, 255, self.camera.scale);
            }
        }
        for (slot, plane) in &self.panning_planes {
            if let Some(texture) = self.surfaces.get(slot) {
                let [x, y] = plane.position;
                graphics::blit(&mut frame, texture, x + shake_x, y + shake_y, None);
            }
        }
        let mut layers = Vec::with_capacity(self.actors.len() + 1);
        for (&index, actor) in &self.actors {
            let (x, y) = actor.shake.as_ref().map_or((0, 0), Shake::offset);
            let (wx, wy) = actor.wave.as_ref().map_or((0, 0), Wave::offset);
            let accessory = u8::try_from(index)
                .ok()
                .filter(|slot| *slot < self.texture_slots.actor_count)
                .and_then(|slot| self.surfaces.get(&(slot + self.texture_slots.accessory)));
            layers.push((
                &actor.layer,
                (shake_x + shift_x + x + wx, shake_y + shift_y + y + wy),
                accessory,
            ));
        }
        if !self.script.dialect.classic() {
            layers.push((&self.cut_in, (0, 0), None));
        }
        layers.sort_by_key(|(layer, ..)| layer.order);
        for (layer, offset, accessory) in layers {
            layer.draw(&mut frame, offset);
            if let Some(texture) = accessory {
                layer.draw_texture(&mut frame, texture, offset);
            }
        }
        if self.script.dialect.classic() {
            // Classic places the cut-in after the actor priority range.
            self.cut_in.draw(&mut frame, (0, 0));
        }
        if let Some(portrait) = &self.gallery_portrait {
            graphics::blit_alpha(&mut frame, portrait, 408, -137, None, 255, 50);
        }
        if let Some(falling) = &self.falling {
            falling.draw(&mut frame);
        }
        if let Some(lens) = &self.lens {
            lens.draw(&mut frame, &self.lens_position);
        }
        if self.fog_enabled
            && let Some(fog) = self.surfaces.get(&self.texture_slots.fog)
        {
            graphics::blit(&mut frame, fog, 0, 0, None);
        }
        if let Some(animation) = &self.animation {
            graphics::blit(
                &mut frame,
                animation.frame(),
                self.animation_position.0,
                self.animation_position.1,
                None,
            );
        }
        if (1..1000).contains(&self.mosaic) {
            graphics::mosaic(frame.raster_mut(), self.mosaic);
        }
        if let Some(transition) = &self.transition {
            transition.draw(&mut frame);
        }
        if let Some(effect) = &self.screen_break {
            effect.draw(&mut frame);
        }
        if let Some(comments) = &self.comments {
            comments.draw(&mut frame, &mut self.text_painter)?;
        }
        if !self.hidden_message
            && let Some(message) = &self.message
        {
            let native_sprites = self
                .templates
                .get(&1)
                .map(|template| template.sprites.as_slice())
                .filter(|sprites| !sprites.is_empty());
            let top = if let Some(sprites) = native_sprites {
                for sprite in sprites {
                    let alpha = self.sprite_alpha(sprite);
                    graphics::blit_alpha(
                        &mut frame,
                        &sprite.source,
                        sprite.x,
                        sprite.y,
                        sprite.clip,
                        alpha,
                        100,
                    );
                }
                sprites
                    .iter()
                    .map(|sprite| sprite.y)
                    .min()
                    .unwrap_or_default()
            } else if let Some(panel) = self.message_panel
                && let Some(texture) = self.surfaces.get(&self.texture_slots.panel)
            {
                let top = height.saturating_sub(panel.height) as i32;
                graphics::blit(&mut frame, texture, 0, top, Some(panel));
                top
            } else {
                (height * 3 / 4) as i32
            };
            let painter = &mut self.text_painter;
            let message_text = |bank| {
                self.templates
                    .get(&bank)
                    .and_then(|template| template.text.last())
            };
            for text in message.text(
                top,
                width,
                self.script.dialect,
                &self.palette,
                [message_text(2), message_text(1)],
                self.text_encoding,
            ) {
                painter.draw(&mut frame, &text)?;
            }
        }
        if self.message_buttons_visible()
            && let Some(bank) = self.message_button_banks.get(&self.active_message_bank)
        {
            for sprite in &bank.artwork.sprites {
                let alpha = self.sprite_alpha(sprite);
                graphics::blit_alpha(
                    &mut frame,
                    &sprite.source,
                    sprite.x,
                    sprite.y,
                    sprite.clip,
                    alpha,
                    100,
                );
            }
            for text in &bank.artwork.text {
                self.text_painter.draw(&mut frame, text)?;
            }
            let hovered = self.hit_button(&bank.buttons, (0, 0));
            draw_buttons(
                &mut frame,
                &bank.buttons,
                hovered,
                true,
                None,
                (0, 0),
                &mut self.text_painter,
                None,
                |_| 0,
            )?;
        }
        if let Some((x, y)) = self
            .mouse_gesture
            .as_ref()
            .and_then(|gesture| gesture.origin)
            && let Some(artwork) = self.templates.get(&15)
        {
            let (dx, dy) = if self.script.dialect.classic() {
                (216, 195)
            } else {
                (90, 103)
            };
            for sprite in &artwork.sprites {
                graphics::blit(
                    &mut frame,
                    &sprite.source,
                    x - dx + sprite.x,
                    y - dy + sprite.y,
                    sprite.clip,
                );
            }
        }
        // The scene visits the script menu bank after the dialogue bank.
        self.draw_script_sprites(&mut frame);
        self.draw_script_controls(&mut frame)?;
        if let Some(offset) = self.backlog_offset {
            self.draw_backlog(&mut frame, offset)?;
        }
        if let Some(fade) = &self.fade {
            let fraction = fade.opacity();
            let [r, g, b] = fade.color;
            if fade.mask.is_none()
                && frame.queue(roukaku_media_core::SceneCommand::rect(
                    "fade",
                    0,
                    0,
                    width,
                    height,
                    [r, g, b, (255.0 * fraction) as u8],
                ))
            {
                return Ok(frame);
            }
            for (x, y, pixel) in frame.raster_mut().enumerate_pixels_mut() {
                let alpha = if let Some((mask, increase)) = &fade.mask {
                    let mx = x * mask.image.width() / width;
                    let my = y * mask.image.height() / height;
                    let threshold = f64::from(mask.image.get_pixel(mx, my)[0]);
                    let amount = (510.0 * fraction - threshold).clamp(0.0, 255.0) as u16;
                    if *increase { 255 - amount } else { amount }
                } else {
                    (255.0 * fraction) as u16
                };
                for (value, color) in pixel.0[..3].iter_mut().zip(fade.color) {
                    *value = ((u16::from(*value) * (255 - alpha) + u16::from(color) * alpha) / 255)
                        as u8;
                }
            }
        }
        Ok(frame)
    }

    fn draw_backlog(&mut self, frame: &mut Canvas, offset: usize) -> Result<()> {
        let classic = self.script.dialect.classic();
        let layout = LogLayout::for_dialect(classic);
        let atlas = self.surfaces.get(&layout.atlas);
        let draw_chip = |frame: &mut Canvas, x, y, sx, sy, width, height| {
            if let Some(atlas) = atlas {
                graphics::blit(
                    frame,
                    atlas,
                    x,
                    y,
                    Some(Clip {
                        x: sx,
                        y: sy,
                        width,
                        height,
                    }),
                );
            }
        };
        let painter = &mut self.text_painter;
        for (row, entry) in self
            .backlog
            .iter()
            .rev()
            .skip(offset)
            .take(layout.rows)
            .rev()
            .enumerate()
        {
            let y = row as i32 * layout.row_pitch;
            let mut text = Text {
                content: entry.line.clone(),
                fixed_pitch: true,
                line_height: None,
                outline: true,
                clip_width: None,
                color: [255; 3],
                x: if classic { 304 } else { 480 },
                y: y + if classic { 76 } else { 104 },
                width: None,
                size: if classic { 24 } else { 22 },
            };
            let voice_area = layout.voice_area(classic, row);
            let hovered = LogLayout::contains(voice_area, self.pointer);
            if classic {
                if !entry.speaker.is_empty() {
                    text.content = format!("{}\n{}", entry.speaker, entry.line);
                }
                if entry.voice.is_some() {
                    draw_chip(frame, 184, y + 114, 148, 0, 67, 55);
                }
            } else {
                draw_chip(frame, 20, y + 96, 0, 220, 1203, 123);
                if entry.voice.is_some() {
                    let [x, y, ..] = voice_area;
                    draw_chip(frame, x, y, if hovered { 492 } else { 362 }, 12, 126, 34);
                }
                if !entry.speaker.is_empty() {
                    let width = self.text_encoding.encode(&entry.speaker).0.len() as i32;
                    let (center, half_cell, size) = if width >= 20 {
                        (75, 6, 12)
                    } else {
                        (48, 8, 16)
                    };
                    painter.draw(
                        frame,
                        &Text {
                            content: entry.speaker.clone(),
                            x: center - half_cell * width / 2,
                            y: y + 82,
                            size,
                            ..text.clone()
                        },
                    )?;
                }
            }
            if hovered && entry.voice.is_some() {
                text.color = self.palette.get(&24).copied().unwrap_or([204; 3]);
            }
            if entry.checkpoint.is_some() {
                let area = layout.jump_area(classic, row);
                let [x, y, ..] = area;
                let hover = LogLayout::contains(area, self.pointer);
                if classic {
                    draw_chip(frame, x, y, 0, if hover { 48 } else { 0 }, 147, 47);
                } else {
                    draw_chip(frame, x, y, if hover { 492 } else { 362 }, 52, 126, 35);
                }
            }
            painter.draw(frame, &text)?;
        }
        let [back_x, back_y, ..] = layout.back;
        let back_hover = LogLayout::contains(layout.back, self.pointer);
        if classic {
            draw_chip(
                frame,
                back_x,
                back_y,
                if back_hover { 48 } else { 0 },
                96,
                48,
                44,
            );
        } else {
            draw_chip(
                frame,
                back_x,
                back_y,
                640,
                if back_hover { 47 } else { 8 },
                104,
                if back_hover { 42 } else { 39 },
            );
        }
        let last = self.backlog.len().saturating_sub(layout.rows);
        if last != 0 || !classic {
            let [x, top, _, bottom] = layout.scrollbar;
            let position = if last == 0 {
                0
            } else {
                (bottom - top) * (last - offset.min(last)) as i32 / last as i32
            };
            let (sx, sy, width, height) = if classic {
                (216, 0, 32, 31)
            } else {
                (1200, 38, 24, 30)
            };
            draw_chip(frame, x, top + position, sx, sy, width, height);
        }
        Ok(())
    }

    fn sprite_alpha(&self, sprite: &Sprite) -> u8 {
        if !sprite.use_window_alpha {
            return 255;
        }
        // Bank 99 exposes this setting as the inverse of the native alpha.
        let setting = self.variable(99, 11).unwrap_or_default();
        ((99i32.saturating_sub(setting).clamp(0, 100) * 255 + 50) / 100) as u8
    }

    fn draw_script_sprites(&self, frame: &mut Canvas) {
        for sprite in &self.overlays {
            let alpha = self.sprite_alpha(sprite);
            match sprite.effect {
                None => graphics::blit_alpha(
                    frame,
                    &sprite.source,
                    sprite.x,
                    sprite.y,
                    sprite.clip,
                    alpha,
                    100,
                ),
                Some(SpriteEffect::Entrance { dx, dy, remaining }) => {
                    let fraction = remaining.div_duration_f64(SPRITE_ENTRANCE);
                    let dx = (f64::from(dx) * fraction).round() as i32;
                    let dy = (f64::from(dy) * fraction).round() as i32;
                    let opacity = ((1.0 - fraction) * f64::from(alpha)).round() as u8;
                    graphics::blit_alpha(
                        frame,
                        &sprite.source,
                        sprite.x + dx,
                        sprite.y + dy,
                        sprite.clip,
                        opacity,
                        100,
                    );
                }
                Some(SpriteEffect::Pulse {
                    amplitude,
                    period,
                    elapsed,
                    ..
                }) => {
                    let phase = elapsed.div_duration_f64(period).fract() * std::f64::consts::TAU;
                    let dx = (phase.sin() * f64::from(amplitude.0)).round() as i32;
                    let dy = (phase.sin() * f64::from(amplitude.1)).round() as i32;
                    graphics::blit_alpha(
                        frame,
                        &sprite.source,
                        sprite.x + dx,
                        sprite.y + dy,
                        sprite.clip,
                        alpha,
                        100,
                    );
                }
            }
        }
    }

    fn draw_script_controls(&mut self, frame: &mut Canvas) -> Result<()> {
        if self.wait == Wait::Menu
            && let Some(buttons) = self
                .choices
                .as_deref()
                .or_else(|| self.parties.get(&self.button_group).map(Vec::as_slice))
        {
            let hovered = (self.button_highlight || self.choices.is_some())
                .then(|| self.keyboard_selection.or_else(|| self.hovered()))
                .flatten()
                .filter(|&index| {
                    buttons
                        .get(index)
                        .is_some_and(|button| button.drag.target == self.dragged_button.is_some())
                });
            draw_buttons(
                frame,
                buttons,
                hovered,
                false,
                self.menu_variant,
                self.active_menu_origin,
                &mut self.text_painter,
                self.slider_drag,
                |slider| {
                    self.variables
                        .get(&slider.binding)
                        .copied()
                        .or_else(|| self.saves.global(slider.binding.0, slider.binding.1))
                        .unwrap_or_default()
                },
            )?;
            if let Some(sprites) = self
                .dragged_button
                .and_then(|index| buttons.get(index))
                .and_then(|button| button.states.get(&2))
            {
                let snap = self.hovered_drop_target().map(|(_, [x, y, ..])| {
                    (x + self.active_menu_origin.0, y + self.active_menu_origin.1)
                });
                for sprite in sprites {
                    let (width, height) = sprite.clip.map_or_else(
                        || sprite.source.image.dimensions(),
                        |clip| (clip.width, clip.height),
                    );
                    let (x, y) = snap.unwrap_or((
                        self.pointer.0 - (width / 2) as i32,
                        self.pointer.1 - (height / 2) as i32,
                    ));
                    graphics::blit(frame, &sprite.source, x, y, sprite.clip);
                }
            }
        }
        if !self.overlay_text.is_empty() {
            let painter = &mut self.text_painter;
            for text in &self.overlay_text {
                painter.draw(frame, text)?;
            }
        }
        Ok(())
    }

    fn step(&mut self) -> Result<()> {
        let script = Arc::clone(&self.script);
        let instruction = script.instruction(self.ip)?;
        self.ip = instruction.next;
        let code = script
            .dialect
            .opcode(instruction.opcode)
            .ok_or_else(|| self.unsupported(instruction))?;
        let tail = if code != 0
            && code != 0x77
            && let Ok(Instruction {
                opcode,
                args: Operands::Words(words),
                next,
                ..
            }) = script.instruction(self.ip)
            && script.dialect.opcode(opcode) == Some(0x77)
        {
            self.ip = next;
            Some(words)
        } else {
            None
        };
        match instruction.args {
            Operands::Pair(args) => self.pair(instruction, code, args),
            Operands::Text { index, bytes } => self.text(instruction, code, index, bytes),
            Operands::Words(args) => self.words(instruction, code, args, tail),
        }
    }

    fn pair(&mut self, instruction: Instruction<'_>, code: u8, [a, b]: [u8; 2]) -> Result<()> {
        match code {
            0x00 => {
                if !self.return_from_script_menu()? {
                    self.wait = Wait::Finished;
                }
            }
            0x02 => {}
            0x03 | 0x04 => {
                self.tick_label = u16::from_le_bytes([a, b]);
                self.tick_step = self.script.read_step(self.tick_label).unwrap_or_default();
                if let Some(scene) = self.sound_names.scenario(&self.script_name) {
                    let read = if code == 0x03 && self.script.dialect.classic() {
                        self.saves.was_read(scene, self.tick_label)?
                    } else {
                        self.saves.mark_read(scene, self.tick_label)?
                    };
                    if code == 0x03
                        && read == Some(false)
                        && self.skip_dialogue
                        && self.variable(99, 30)? != 0
                    {
                        self.skip_dialogue = false;
                        self.frame_dirty = true;
                    }
                }
            }
            0x05 => self.control_music_volume(a)?,
            0x06 => {
                if !self
                    .comments
                    .as_ref()
                    .is_some_and(|comments| comments.suppress_dialogue_wait)
                {
                    self.wait = Wait::Input;
                    self.frame_dirty = self.background.is_some();
                }
                self.record_current_dialogue()?;
            }
            0x07 => self.wait = Wait::VoiceTail(Duration::from_secs(1)),
            0x09 => {
                let base = self
                    .parties
                    .get(&3)
                    .into_iter()
                    .flatten()
                    .filter(|button| button.label.is_empty())
                    .cloned()
                    .collect();
                self.choices = Some(base);
            }
            0x0a => {
                if self
                    .choices
                    .as_ref()
                    .is_some_and(|buttons| buttons.iter().any(Button::selectable))
                {
                    if self.variable(99, 210)? != 0 {
                        self.skip_dialogue = false;
                    }
                    if self.variable(99, 209)? != 0 {
                        self.auto_dialogue = false;
                    }
                    self.wait = Wait::Menu;
                    self.active_menu_origin = (0, 0);
                    self.keyboard_selection = None;
                    self.frame_dirty = true;
                } else {
                    self.choices = None;
                }
            }
            0x0b => {
                self.refresh_scroll_list(self.variable(99, 246)?)?;
                self.wait = if self
                    .parties
                    .get(&self.button_group)
                    .is_some_and(|buttons| !buttons.is_empty())
                {
                    Wait::Menu
                } else {
                    Wait::Input
                };
                self.frame_dirty = true;
            }
            0x0c => {
                self.open_menu(self.button_group, None);
                self.refresh_scroll_list(self.variable(99, 246)?)?;
            }
            0x0f if matches!(a, 7 | 8 | 10 | 11 | 13 | 14) => {}
            0x0f if matches!(a, 0 | 1) => self.fade_in(b)?,
            0x0f if a == 3 => {
                self.fade_color = [255; 3];
                if self.variable(99, 10)? != 0 {
                    let total = Fade::FLASH_PERIOD * (u32::from(b) + 1);
                    self.fade = Some(Fade {
                        total,
                        remaining: total,
                        color: self.fade_color,
                        mask: None,
                        pulse: true,
                    });
                    self.frame_dirty = true;
                }
            }
            0x0f if matches!(a, 4 | 6) => {
                self.shake(None, a == 6, a == 6 || self.script.dialect.classic());
            }
            0x0f if a == 5 => self.frame_dirty |= self.background.is_some(),
            0x0f if a == 9 => {
                self.transition_backdrop = Some(self.compose()?.into_raw().into());
            }
            0x0f if matches!(a, 15 | 16) => {
                let captured = self.transition_backdrop.take();
                if self.control_keys == 0 && !self.skip_dialogue && self.variable(99, 10)? != 0 {
                    let resource = if self.script.dialect.version == 0x9e97 {
                        r"SYSTEM\EFF"
                    } else {
                        r"SYSTEM\EFFBREAK"
                    };
                    let mask = self.graphics.load(&self.game, resource)?;
                    let frame = match captured.or_else(|| self.last_frame.clone()) {
                        Some(frame) => frame,
                        None => self.compose()?.into_raw().into(),
                    };
                    self.screen_break = Some(ScreenBreak::new(
                        frame,
                        self.extent,
                        mask,
                        b,
                        &mut self.random,
                    ));
                    self.frame_dirty = true;
                }
            }
            0x0f if a == 17 => {
                if self.control_keys == 0 && !self.skip_dialogue && self.variable(99, 10)? != 0 {
                    let name: &[u8] = if b == 1 {
                        b"MSK\\MASK11A"
                    } else {
                        b"MSK\\MASK04A"
                    };
                    self.load_surface(self.texture_slots.mask, name)?;
                    self.mask_fade(5, false)?;
                }
            }
            0x0f if matches!(a, 20 | 21) => self.mask_fade(b, a == 20)?,
            0x0f if matches!(a, 23..=25 | 30..=32 | 40) => {
                self.frame_dirty |= self.background.is_some();
            }
            0x0f if matches!(a, 51 | 52) => {
                for actor in 0..5 {
                    if b & (1 << actor) != 0 {
                        self.shake(Some(actor), a == 52, a == 52);
                    }
                }
            }
            0x0f if a == 2 => {
                self.parties.remove(&0);
                self.overlays.clear();
                self.overlay_text.clear();
                if b != 0 {
                    self.fade_in(b)?;
                }
            }
            0x10 => self.wait = Wait::Finished,
            0x15 if a == 0xff => {
                self.wait = Wait::Input;
                self.frame_dirty = true;
            }
            0x15 if a == 0xfe => {
                self.wait = Wait::Motion;
                self.frame_dirty = true;
            }
            0x15 => {
                self.wait = if a == 0 && b != 0 {
                    Wait::Input
                } else {
                    Wait::Timer {
                        remaining: Duration::from_millis(u64::from(a) * 200),
                        skippable: b != 0,
                    }
                };
                self.frame_dirty = true;
            }
            0x16 => {
                let mut flags = a;
                while let Ok(next) = self.script.instruction(self.ip) {
                    if self.script.dialect.opcode(next.opcode) != Some(0x16) {
                        break;
                    }
                    let Operands::Pair([more, _]) = next.args else {
                        return Err(Error::Invalid("ADV32 audio wait operand"));
                    };
                    flags |= more;
                    self.ip = next.next;
                }
                self.wait = Wait::Audio {
                    flags,
                    start_timeout: Duration::from_secs(1),
                };
            }
            0x11 => {
                if a == 0 {
                    self.variables.retain(|&(bank, _), _| bank > 10);
                    self.player_names = self.saves.player_names()?;
                }
                self.message = None;
                self.speaker.clear();
                self.tick_label = 0;
            }
            0x17 => self.set_button_sounds([u16::from(a), u16::from(b)]),
            0x13 => self.stop_music()?,
            0x18 if a == 0 => {
                let slot = self.variable(99, 254)? as u16;
                self.load_slot(slot)?;
            }
            0x18 if matches!(a, 2 | 3) => {
                self.window.push_back(WindowCommand::SetVisibility(a == 3))
            }
            0x18 if a == 4 => {
                let slot = if b == 0 {
                    self.variable(99, 254)? as u16
                } else {
                    u16::from(self.script.dialect.classic())
                };
                self.save_slot(slot)?;
            }
            0x18 if a == 5 => self.saves.flush_globals()?,
            0x18 if a == 6 => {
                if let Some(scene) = self.sound_names.scenario(&self.script_name) {
                    self.saves.mark_read(scene, self.tick_label)?;
                }
            }
            0x18 if a == 7 => self.display_preference = b,
            0x18 if a == 8 => self.set_fullscreen(b != 0),
            0x18 if a == 10 => {
                if !self.skip_disc_check {
                    crate::authorization::check_disc(&self.game.data)?;
                }
            }
            0x18 if a == 15 => self.input_mode = b,
            0x18 if a == 18 => self.menu_layers_enabled = true,
            0x18 if a == 20 => {
                self.saves.reset_settings()?;
                self.refresh_saved_preferences()?;
            }
            // These operations access the optional native cloud account.
            0x18 if matches!(a, 22 | 23 | 25) && self.script.dialect.version == 0x9f9a => {}
            0x18 if a == 24 => {
                let slot = if b == 0 {
                    self.variable(99, 254)? as u16
                } else {
                    u16::from(b)
                };
                if let Some(menu) = &self.script_menu {
                    self.saves.write_preliminary_slot(slot, &menu.story)?;
                } else {
                    self.saves.write_preliminary_slot(slot, &self.snapshot()?)?;
                }
            }
            0x18 if a == 26 => {
                self.saves.reload_globals()?;
                self.variables
                    .retain(|&(bank, index), _| !self.saves.is_global(bank, index));
            }
            0x18 if a == 27 => {
                #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
                self.window.push_back(WindowCommand::SetClientSize {
                    width: self.extent.0,
                    height: self.extent.1,
                });
                self.frame_dirty = true;
            }
            0x18 if a == 29 => {
                self.saves.reset_preferences(b)?;
                self.refresh_saved_preferences()?;
            }
            0x18 if matches!(a, 1 | 9 | 11..=14 | 16..=17 | 19 | 99) => {}
            0x1a if a == 0 => {
                self.parties.remove(&0);
                self.button_group = 0;
            }
            0x1a => {
                let buttons = self.parties.remove(&self.button_group).unwrap_or_default();
                if matches!(a, COMPACT_MESSAGE_BANK | EXPANDED_MESSAGE_BANK) {
                    self.message_button_banks.insert(
                        a,
                        MessageButtonBank {
                            buttons,
                            artwork: Template {
                                sprites: std::mem::take(&mut self.overlays),
                                text: std::mem::take(&mut self.overlay_text),
                            },
                        },
                    );
                } else {
                    self.parties.insert(a, buttons);
                }
                self.button_group = 0;
            }
            0x1c => {
                let marker = self.script.instruction(self.ip)?;
                if !matches!(marker.args, Operands::Text { index: None, .. }) {
                    return Err(Error::Invalid("ADV32 resume marker operand"));
                }
                self.resume_offset = Some(marker.offset);
                self.ip = marker.next;
            }
            0x1d => {
                if b == 0 {
                    self.skip_dialogue = false;
                    self.auto_dialogue = false;
                }
                self.return_from_script_menu()?;
            }
            0x1e if a == 1 => {
                let now = time::OffsetDateTime::now_local()
                    .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
                self.set_variable(3, 0, i32::from(now.hour()))?;
            }
            0x1e if a == 2 => {
                let mut best = 0;
                let mut score = self.variable(5, 0)?;
                for index in 1..=3 {
                    let candidate = self.variable(5, index)?;
                    if candidate > score {
                        best = i32::from(index);
                        score = candidate;
                    }
                }
                self.set_variable(3, 0, best)?;
            }
            0x1e if a == 18 => {
                if let Some(changed) = self.saves.preference_mark(b) {
                    self.set_variable(3, 0, i32::from(changed))?;
                } else if b > 1 {
                    self.refresh_saved_preferences()?;
                }
            }
            0x1e if a == 19 => {
                let slot = self.variable(99, 254)? as u16;
                self.set_variable(9, 87, i32::from(!self.saves.exists(slot)))?;
            }
            0x1e if a == 20 => {
                self.slot_thumbnails.clear();
                if b == 0 {
                    let slots = self.save_grid_slots()?;
                    for slot in slots {
                        if let Some(image) = self.saves.thumbnail(slot)? {
                            self.slot_thumbnails
                                .insert(slot, Arc::new(Crx::from_image(0, 0, image)));
                        }
                    }
                }
            }
            0x1e if a == 6 => self.queue_save_grid(b != 0)?,
            0x1e if a == 7 => {
                if b == 0 && self.backlog_offset.is_some() {
                    self.wait = Wait::Input;
                    self.frame_dirty = true;
                }
            }
            0x1e if a == 9 && (b == 0 || (b == 1 && self.script.dialect.version == 0x9f9a)) => {
                self.start_staff_roll(b != 0)?
            }
            0x1e if a == 51 && b == 1 && !self.script.dialect.classic() => {
                self.custom_names = self.saves.uses_custom_player_names(&self.player_names)?;
            }
            0x1e if a == 51 && b == 0 => {
                let backdrop = match self.graphics.load(&self.game, "SYSTEM\\NAMEBACK") {
                    Ok(backdrop) => Some(backdrop),
                    Err(Error::MissingAsset(_)) => None,
                    Err(error) => return Err(error),
                };
                self.name_entry = Some(NameEntry {
                    names: self.player_names.clone(),
                    given_selected: false,
                    backdrop,
                });
                self.wait = Wait::NameEntry;
                self.frame_dirty = true;
            }
            // Completion reporting is optional and requires a registered name and email.
            0x1e if a == 105 => {}
            // These selectors leave the extension state unchanged in the modern dispatcher.
            0x1e if matches!((a, b), (100 | 215, 0)) && !self.script.dialect.classic() => {}
            0x1e if a == 200 => self.music_room(b)?,
            0x1e if a == 201 || a == 202 => self.music_favorite(a == 201, b)?,
            0x1e if a == 203 && self.script.dialect.version == 0x9f9a => {
                self.show_gallery_portrait()?
            }
            0x1e if a == 204 && self.script.dialect.version == 0x9f9a => {
                self.check_gallery_page(b)?
            }
            0x1e if a == 215 && self.script.dialect.version == 0x9f9a => {}
            0x20 if a == 0 => self.skip_block(b)?,
            0x20 => {}
            // The CPU compositor already applies changes to staged objects.
            0x22 => self.frame_dirty = true,
            // Unassigned template selectors leave the current artwork alone.
            0x24 if matches!(a, 0 | 8..=12 | 17..=253) => {}
            0x24 => {
                // Sprite and text templates share selector numbers with the
                // interactive banks registered by 0x1a.
                self.parties.remove(&0);
                let template = Template {
                    sprites: std::mem::take(&mut self.overlays),
                    text: std::mem::take(&mut self.overlay_text),
                };
                self.templates.insert(a, template);
                self.button_group = 0;
            }
            0x25 if a != 0 => {
                // ADV32 uses the 15-bit MSVC runtime rand() output.
                let value = rand15(&mut self.random) % u32::from(a);
                self.set_variable(99, 2, value as i32)?;
            }
            // Reserved in the native dispatch table.
            0x27 => {}
            0x28 if a == 0 => {
                self.frame_dirty |= self.falling.take().is_some();
                self.particle_region = ParticleRegion::default();
                self.frame_dirty |= self.lens.take().is_some();
                self.frame_dirty |= std::mem::take(&mut self.fog_enabled);
            }
            0x28 if a == 7 => {
                self.fog_enabled = b != 0;
                self.frame_dirty = true;
            }
            0x28 if a == 15 => self.set_lens(b)?,
            0x28 if matches!(a, 1..=6 | 8..=13 | 17) => {
                let texture = match a {
                    2 => self.surface(self.texture_slots.rain)?,
                    13 => {
                        let resource = if self.script.dialect.version == 0x9e97 {
                            "SYSTEM\\YOUSEI"
                        } else {
                            "SYSTEM\\EFFYOUSEI"
                        };
                        self.graphics.load(&self.game, resource)?
                    }
                    17 => self.graphics.load(&self.game, "SYSTEM\\EFFHOTARU")?,
                    _ => self.surface(self.texture_slots.falling)?,
                };
                self.falling = Some(FallingSprites::new(
                    texture,
                    self.extent,
                    b,
                    match a {
                        2 => ParticleKind::Rain,
                        3 => ParticleKind::FastFalling,
                        13 | 17 => ParticleKind::Drifting,
                        _ => ParticleKind::Falling,
                    },
                    self.particle_region,
                    &mut self.random,
                ));
                self.frame_dirty = true;
            }
            0x32 => self.button_highlight = true,
            0x33 => {
                let button = self
                    .parties
                    .get_mut(&self.button_group)
                    .and_then(|buttons| buttons.last_mut())
                    .ok_or(Error::Invalid("ADV32 drag role outside a button"))?;
                if a == 0 {
                    button.drag.source = true;
                } else {
                    button.drag.target = true;
                }
            }
            0x34 if matches!(a, 0 | 1 | 4) => {
                let button = self
                    .parties
                    .get_mut(&self.button_group)
                    .and_then(|buttons| buttons.last_mut())
                    .ok_or(Error::Invalid("ADV32 button option outside a button"))?;
                match a {
                    0 => button.hit_test_alpha = true,
                    1 => button.music_slider = true,
                    _ => button.draw_on_top = true,
                }
            }
            0x34 if a < 16 => self.button_options |= 1 << a,
            0x35 => self.button_highlight = false,
            0x36 => self.finish_scene_motion()?,
            0x37 => self.center_next_background = true,
            0x38 if self.script.dialect.classic() => {
                if self.control_keys == 0 && !self.skip_dialogue {
                    let texture = self.surface(self.texture_slots.plane + 1)?;
                    self.transition = Some(if b == 0 {
                        SceneTransition::classic_sweep(texture, a)
                    } else {
                        SceneTransition::new(texture, true, a, &mut self.random)
                    });
                    self.frame_dirty = true;
                }
            }
            0x38 if matches!(a, 1 | 2) => {
                self.transition = Some(SceneTransition::new(
                    self.surface(22)?,
                    a == 1,
                    b,
                    &mut self.random,
                ));
                self.frame_dirty = true;
            }
            0x38 if a == 0 => self.frame_dirty |= self.transition.take().is_some(),
            _ => return Err(self.unsupported(instruction)),
        }
        Ok(())
    }

    fn text(
        &mut self,
        instruction: Instruction<'_>,
        code: u8,
        index: Option<u8>,
        bytes: &[u8],
    ) -> Result<()> {
        match (code, index) {
            (0x3c, Some(0)) => {}
            (0x3c, Some(1)) => {
                self.load_surface(1, bytes)?;
                if bytes.windows(8).any(|name| {
                    name.eq_ignore_ascii_case(b"BG\\BLACK")
                        || name.eq_ignore_ascii_case(b"BG\\WHITE")
                }) {
                    self.preserve_view = false;
                }
                if !self.preserve_view {
                    self.camera = Camera {
                        scale: 100,
                        center: (0, 0),
                    };
                    self.camera_motion = None;
                    self.scene_translation = (0, 0);
                    self.scene_translation_motion = None;
                }
            }
            (0x3c, Some(slot)) => self.load_surface(slot, bytes)?,
            (0x3d, Some(index @ 0..=5)) => {
                self.load_surface(self.texture_slots.panel + index, bytes)?
            }
            (0x3f, Some(index)) if index < self.texture_slots.actor_count => {
                self.show_actor(index, bytes)?
            }
            (0x40, Some(index)) => {
                match bytes {
                    b"-" => self.cut_in.visible = false,
                    b"-a" => self.cut_in.visible = index != 0,
                    b"-m" => self.cut_in.transition(
                        Duration::from_millis(200 * u64::from(index)),
                        0,
                        false,
                        None,
                    ),
                    b"-n" => self.cut_in.finish(),
                    b"--" | b"." => {
                        self.cut_in.texture =
                            self.surfaces.get(&self.texture_slots.cut_in).cloned();
                        self.cut_in.visible = self.cut_in.texture.is_some();
                    }
                    _ => {
                        self.cut_in.texture =
                            Some(self.graphics.load(&self.game, &decode_japanese(bytes)?)?);
                        self.cut_in.visible = true;
                    }
                }
                self.frame_dirty = true;
            }
            (0x41, Some(actor)) if actor < self.texture_slots.actor_count => {
                let slot = actor + self.texture_slots.accessory;
                match bytes {
                    b"-" => self.load_surface(slot, bytes)?,
                    b"--" => {
                        if let Some(name) = self.previous_accessories.get(&slot).cloned() {
                            self.load_surface(slot, &name)?;
                        }
                    }
                    _ => {}
                }
                self.frame_dirty = true;
            }
            (0x43, Some(index)) => {
                self.button_routes.insert(index, bytes.to_vec());
            }
            (0x46, Some(index)) => {
                if index == 255 || bytes == b"-" {
                    self.sound_pan_prefixes.clear();
                    if index == 255 {
                        self.set_sound_pan(2, 0.0)?;
                    }
                } else {
                    let prefix = decode_japanese(bytes)?.to_ascii_uppercase();
                    if !self
                        .sound_pan_prefixes
                        .iter()
                        .any(|(stored, _)| stored.starts_with(&prefix))
                    {
                        self.sound_pan_prefixes
                            .push((prefix, f32::from(index as i8) / 100.0));
                    }
                }
            }
            (0x47, Some(_)) => {
                if let Some((actor, x, y)) = self.actor_anchor {
                    let name = decode_japanese(bytes)?.to_ascii_uppercase();
                    self.actor_anchors.insert((actor, name), (x, y));
                }
            }
            (0x49, None) if bytes == b":n" => self.preserve_view = true,
            (0x49, None) if bytes == b":r" => self.preserve_view = false,
            (0x49, None) => {
                let name = self.picture_name(bytes)?;
                self.background = if matches!(name.as_str(), "-" | "--") {
                    self.surfaces.get(&1).cloned()
                } else {
                    Some(self.graphics.load(&self.game, &name)?)
                };
                if let Some(picture) = self.sound_names.picture_index(&name) {
                    self.set_variable(97, picture as u16, 1)?;
                }
                if !self.preserve_view {
                    self.background_position = (0, 0);
                    self.pan = None;
                }
                self.centered_background = std::mem::take(&mut self.center_next_background);
                self.fitted_background = None;
                // A native background commit releases the previous actors and cut-in.
                let actors = self.texture_slots.actor
                    ..self.texture_slots.accessory + self.texture_slots.actor_count;
                for slot in actors.chain(std::iter::once(self.texture_slots.cut_in)) {
                    self.load_surface(slot, b"-")?;
                }
                self.actors.clear();
                self.cut_in = SceneLayer::default();
                self.scene_shake = None;
                self.fade = None;
                self.screen_break = None;
                self.mosaic = 0;
                self.mosaic_tween = None;
                self.overlays.clear();
                self.overlay_text.clear();
                self.frame_dirty = true;
            }
            (0x4b, None) => {}
            (0x4c | 0x4d | 0x4e, None) => {
                let name = decode_japanese(bytes)?;
                if name == "-" {
                    self.pending_voice = None;
                    self.stop_sound(1)?;
                } else {
                    self.pending_voice = Some(name.clone());
                    if self.control_keys == 0 && !self.skip_dialogue {
                        self.play_sound(&name, 1, false)?;
                        self.last_voice = Some(name);
                    }
                }
            }
            (0x4f | 0x50, None) => {
                if let Some(scene) = self.sound_names.scenario(&self.script_name) {
                    self.set_variable(98, scene as u16, 1)?;
                }
                self.switch_script(bytes)?;
            }
            (0x51, None) => self.next_label = Some(bytes.to_vec()),
            #[cfg(feature = "video-ffmpeg")]
            (0x52, None) => {
                if self.variable(99, 202)? == 0 {
                    self.start_movie(&decode_japanese(bytes)?)?;
                }
            }
            (0x54, None) => self.jump(bytes)?,
            (0x58, None) => {
                if let Some(item) = self
                    .scroll_list
                    .as_mut()
                    .and_then(|list| list.items.last_mut())
                {
                    item.label = bytes.to_vec();
                } else {
                    self.menu_layers_enabled = !bytes.is_empty();
                }
            }
            (0x59, None) => {
                if self.variable(93, self.menu_position.2)? != 0
                    && let Some(label) = self.button_routes.get(&0).cloned()
                {
                    self.jump(&label)?;
                } else {
                    self.open_confirmation(2, Confirmation::Remember(self.menu_position.2));
                }
            }
            (0x5a, None) => self.open_confirmation(16, Confirmation::Prompt),
            (0x5b, None) => self.open_confirmation(17, Confirmation::Prompt),
            (0x5c, None) => {
                let line = self.script.instruction(self.ip)?;
                let Operands::Text { bytes: raw, .. } = line.args else {
                    return Err(Error::Invalid("ADV32 backlog entry"));
                };
                self.ip = line.next;
                let caption = Script::decode_text(raw, self.text_encoding)?;
                if self.script.dialect.version == 0x9f9a {
                    self.classic_choice_text
                        .insert(bytes.to_vec(), caption.clone());
                }
                if self.choices.is_some() {
                    self.add_choice(bytes, caption);
                } else {
                    self.log_message(caption, decode_japanese(bytes)?, None, None);
                }
            }
            // The script compiler emits this annotation without a runtime action.
            (0x5d, None) => {}
            (0x61, None) => {
                self.music_end_route = (!bytes.is_empty()).then(|| bytes.to_vec());
                self.music_idle = Duration::ZERO;
            }
            (0x63, None) if self.metadata.len() + bytes.len() < 128 => {
                self.metadata.push_str(&decode_japanese(bytes)?);
            }
            (0x63, None) => {}
            (0x64, None) => {
                if bytes == b"-" {
                    self.load_surface(31, bytes)?;
                    self.load_surface(32, bytes)?;
                }
            }
            (0x65, None) => {
                if bytes.is_empty() {
                    self.voice_flags.clear();
                } else {
                    self.voice_flags.push(decode_japanese(bytes)?);
                }
            }
            (0x68, None) => {
                if bytes == b"-" {
                    self.cursor = None;
                } else {
                    let name = decode_japanese(bytes)?;
                    let cursor = self.game.resource("grp/cursor", &name, "cur")?;
                    read_bounded(cursor, 1024 * 1024)?;
                    self.cursor = Some(name);
                }
            }
            (0x6a, None) => {
                self.speaker =
                    Script::decode_dialogue(bytes, self.text_encoding, self.script.dialect)?;
            }
            (0x6b, None) => {
                let decoded =
                    Script::decode_dialogue(bytes, self.text_encoding, self.script.dialect)?;
                let message = Message::new(
                    decoded,
                    std::mem::take(&mut self.speaker),
                    &self.player_names,
                    self.custom_names,
                    self.script.dialect.version == 0x9f9a,
                )?;
                self.message = Some(message);
                self.pending_log_entry = true;
                self.message_offset = Some(instruction.offset);
                self.frame_dirty = true;
                if self
                    .script
                    .instruction(self.ip)
                    .ok()
                    .and_then(|next| self.script.dialect.opcode(next.opcode))
                    != Some(0x06)
                {
                    self.wait = Wait::Input;
                    self.record_current_dialogue()?;
                }
            }
            // Text operands are consumed by their owning instruction.
            (0x6c, None) if bytes.is_empty() => {}
            (0x6d, None) => {
                if let Some(comments) = &mut self.comments {
                    comments.append(bytes, self.text_encoding)?;
                }
            }
            _ => return Err(self.unsupported(instruction)),
        }
        Ok(())
    }

    fn words(
        &mut self,
        instruction: Instruction<'_>,
        code: u8,
        a: [u16; 4],
        tail: Option<[u16; 4]>,
    ) -> Result<()> {
        let signed = a.map(|value| i32::from(value as i16));
        if self.recording_motion && matches!(code, 0x89 | 0x8b | 0x8c | 0x8e | 0x9d) {
            self.recorded_motion
                .push((self.script.clone(), instruction.offset));
        }
        match code {
            0x6f | 0x70 if a[0] == 255 => self.stop_music()?,
            0x6f | 0x70 => {
                self.control_music_volume(255)?;
                let continuing = code == 0x70 && a[1] == 1 && self.background_music == Some(a[0]);
                if !continuing {
                    self.play_music_track(a[0], a[1] <= 1)?;
                }
            }
            0x71 if a[0] == 0xfffe => self.stop_sound(1)?,
            0x71 if a[0] == 0xffff => {
                for channel in 2..=5 {
                    self.stop_sound(channel)?;
                }
            }
            0x71 => {
                self.play_effect(a[0], a[1] != 0)?;
            }
            0x72 => {
                let (label, next) = match self.script.instruction(self.ip)? {
                    Instruction {
                        args: Operands::Text { index: None, bytes },
                        next,
                        ..
                    } => (bytes.to_vec(), next),
                    _ => return Err(Error::Invalid("ADV32 menu label")),
                };
                self.ip = next;
                self.parties
                    .entry(self.button_group)
                    .or_default()
                    .push(Button {
                        area: signed,
                        label,
                        ..Button::default()
                    });
            }
            0x76 => {
                let operand = self.script.instruction(self.ip)?;
                let Operands::Text { bytes, .. } = operand.args else {
                    return Err(Error::Invalid("ADV32 menu caption operand"));
                };
                let caption = self
                    .text_encoding
                    .decode_without_bom_handling_and_without_replacement(bytes)
                    .ok_or(Error::Invalid("ADV32 menu caption encoding"))?;
                let (content, size) = self.caption_style(caption.into_owned());
                self.ip = operand.next;
                if !content.is_empty() {
                    let palette = (a[2] & 0x7f) + 12;
                    let color = self.palette.get(&palette).copied().unwrap_or([255; 3]);
                    let caption = Text {
                        fixed_pitch: true,
                        line_height: None,
                        outline: a[2] > 128,
                        clip_width: None,
                        content,
                        x: signed[0],
                        y: signed[1],
                        size,
                        color,
                        width: None,
                    };
                    let button = self
                        .parties
                        .get_mut(&self.button_group)
                        .and_then(|buttons| buttons.last_mut())
                        .ok_or(Error::Invalid("ADV32 caption outside a button"))?;
                    button.captions.entry(a[3]).or_default().push(caption);
                }
            }
            0x75 if !self.menu_layers_enabled
                && self.script.dialect.version != 0x9f9a
                && self
                    .scroll_list
                    .as_ref()
                    .is_none_or(|list| list.items.is_empty()) => {}
            0x73 if let Some([variable, _, _, 5]) = tail => {
                let [start, end, positions, bank] = signed;
                if start == end || positions <= 1 || bank < 0 {
                    return Err(Error::Invalid("ADV32 slider range"));
                }
                let button = self
                    .parties
                    .get_mut(&self.button_group)
                    .and_then(|buttons| buttons.last_mut())
                    .ok_or(Error::Invalid("ADV32 slider outside a button"))?;
                button.slider = Some(Slider {
                    start,
                    end,
                    last_value: positions - 1,
                    binding: (bank as u16, variable),
                });
            }
            0x73 | 0x74 | 0x75 | 0x7a => {
                let Some(tail) = tail else {
                    return Err(Error::Invalid("ADV32 blit operands"));
                };
                let slot = if code == 0x7a {
                    tail[2] & 0x3f
                } else {
                    tail[2]
                };
                let slot =
                    u8::try_from(slot).map_err(|_| Error::Invalid("ADV32 surface number"))?;
                let Some(source) = self.surfaces.get(&slot).cloned() else {
                    // The sprite draw call has no surface to compose before its layer is loaded.
                    return Ok(());
                };
                let offset = if code == 0x7a {
                    self.blit_offset
                } else {
                    (0, 0)
                };
                let sprite = Sprite {
                    source,
                    x: signed[0] + offset.0,
                    y: signed[1] + offset.1,
                    clip: Some(Clip {
                        x: signed[2],
                        y: signed[3],
                        width: u32::from(tail[0]),
                        height: u32::from(tail[1]),
                    }),
                    use_window_alpha: code == 0x7a && tail[2] & 0x80 != 0,
                    effect: None,
                };
                if code == 0x73 {
                    let button = self
                        .parties
                        .get_mut(&self.button_group)
                        .and_then(|buttons| buttons.last_mut())
                        .ok_or(Error::Invalid("ADV32 menu image outside a button"))?;
                    button.states.entry(tail[3]).or_default().push(sprite);
                } else if code == 0x75
                    && tail[3] != u16::MAX
                    && let Some(item) = self
                        .scroll_list
                        .as_mut()
                        .and_then(|list| list.items.last_mut())
                {
                    item.states.entry(tail[3]).or_default().push(sprite);
                } else {
                    self.overlays.push(sprite);
                    self.frame_dirty = true;
                }
            }
            0x78 => {
                let from = (signed[0], signed[1]);
                let to = (signed[2], signed[3]);
                self.active_menu_origin = from;
                if from != to {
                    let frames = if self.script.dialect.version == 0x9e97 {
                        60
                    } else {
                        20
                    };
                    let total = Duration::from_micros(frames * 1_000_000 / 60);
                    self.menu_pan = Some(Pan {
                        from,
                        to,
                        total,
                        remaining: total,
                    });
                } else {
                    self.menu_pan = None;
                }
            }
            0x7b if a[1] == 768 => {
                if let Some(sprite) = self.overlays.last_mut() {
                    let (dx, dy) = match a[2] {
                        0 => (-100, 0),
                        1 => (100, 0),
                        2 => (0, -100),
                        3 => (0, 100),
                        _ => (0, 0),
                    };
                    sprite.effect = Some(SpriteEffect::Entrance {
                        dx,
                        dy,
                        remaining: SPRITE_ENTRANCE,
                    });
                }
            }
            0x7b if a[1] <= 256 => {
                if a[0] & 0x0f <= 1
                    && a[1] != 0
                    && let Some(sprite) = self.overlays.last_mut()
                {
                    let amplitude = if a[0] & 0x0f == 0 {
                        (signed[2], 0)
                    } else {
                        (0, signed[2])
                    };
                    sprite.effect = Some(SpriteEffect::Pulse {
                        amplitude,
                        period: Duration::from_micros(2 * u64::from(a[1]) * 1_000_000 / 60),
                        elapsed: Duration::ZERO,
                        repeat: a[0] & 0x0f00 != 0,
                    });
                }
            }
            0x79 => self.menu_position = (signed[0], signed[1], a[3]),
            0x7d => {
                self.slot_rectangles.clear();
                self.scroll_list = Some(ScrollList {
                    bounds: signed,
                    items: Vec::new(),
                });
            }
            0x7e => self.slot_rectangles.push(signed),
            0x7f => {
                if let Some(list) = &mut self.scroll_list {
                    list.items.push(Button {
                        list_index: Some(a[0]),
                        ..Button::default()
                    });
                }
            }
            0x80 if self.script.dialect.version == 0x9f9a => {
                let operand = self.script.instruction(self.ip)?;
                let Operands::Text { bytes, .. } = operand.args else {
                    return Err(Error::Invalid("ADV32 list caption operand"));
                };
                self.ip = operand.next;
                let caption = self
                    .text_encoding
                    .decode_without_bom_handling_and_without_replacement(bytes)
                    .ok_or(Error::Invalid("ADV32 list caption encoding"))?;
                if let Some(item) = self
                    .scroll_list
                    .as_mut()
                    .and_then(|list| list.items.last_mut())
                {
                    item.captions.insert(
                        99,
                        vec![Text {
                            fixed_pitch: false,
                            line_height: None,
                            outline: false,
                            clip_width: None,
                            content: caption.into_owned(),
                            x: signed[0],
                            y: signed[1],
                            size: 22,
                            color: [255; 3],
                            width: None,
                        }],
                    );
                }
            }
            0x81 => self.set_button_sounds([a[0], a[1]]),
            0x82 if a[3] <= 2 => {
                let value = match a[3] {
                    1 => self.variable(a[0], a[1])?.wrapping_add(signed[2]),
                    2 => self.variable(a[0], a[1])?.wrapping_sub(signed[2]),
                    _ => signed[2],
                };
                self.set_variable(a[0], a[1], value)?;
            }
            0x83 => {
                let source = self.variable(a[2], a[3])?;
                self.set_variable(a[0], a[1], source)?;
            }
            0x84 => {
                let mut sum = 0i32;
                loop {
                    let operand = self.script.instruction(self.ip)?;
                    if operand.opcode == 0x02 {
                        break;
                    }
                    let Operands::Words([bank, index, ..]) = operand.args else {
                        return Err(Error::Invalid("ADV32 sum operand"));
                    };
                    sum = sum.wrapping_add(self.variable(bank, index)?);
                    self.ip = operand.next;
                }
                self.set_variable(a[0], a[1], sum)?;
            }
            0x85 => {
                let input = self.variable(a[2], a[3])?;
                let value = self.variable(a[0], a[1])?.wrapping_add(input);
                self.set_variable(a[0], a[1], value)?;
            }
            0x86 => self.compare(a)?,
            0x87 if a[2] == 0 => {}
            0x87 => self.queue_overlay_text(a)?,
            0x88 => self.blit_offset = (signed[0], signed[1]),
            0x89 => self.pan_to((signed[0], signed[1]), a[2], a[3]),
            0x8b if a[3] == 0 => {
                let actor = self.actors.entry(a[2]).or_default();
                actor.layer.position = (signed[0], signed[1]);
                actor.motion = None;
                actor.next_motion.clear();
                actor.relative_origin = None;
                self.frame_dirty |= actor.layer.visible;
            }
            0x8b if a[3] == 255 => {}
            0x8b if (a[3] as u8 as i8) < 0 => {
                let actor = self.actors.entry(a[3] & 0x0f).or_default();
                let duration = if self.control_keys != 0 || self.skip_dialogue {
                    Duration::ZERO
                } else {
                    Duration::from_millis(200 * u64::from(a[1]))
                };
                actor.layer.transition(
                    duration,
                    0,
                    true,
                    self.surfaces.get(&self.texture_slots.mask).cloned(),
                );
                self.frame_dirty = true;
            }
            0x8b if a[3] & 0x40 != 0 && a[0] <= 3 => {
                let (frames, offset) = match (self.script.dialect.version, a[0]) {
                    (0x9f9a, 0) => (20, -20),
                    (0x9f9a, 1) => (20, 20),
                    (0x9f9a, 2) => (20, 0),
                    (_, 0) => (12, 0),
                    (_, 1) => (12, -20),
                    (_, 2) => (12, 20),
                    _ => (12 * u64::from(a[1]), 0),
                };
                let duration = if self.control_keys != 0 || self.skip_dialogue {
                    Duration::ZERO
                } else {
                    Duration::from_micros(frames * 1_000_000 / 60)
                };
                let mask = (a[0] == 3)
                    .then(|| self.surfaces.get(&self.texture_slots.mask).cloned())
                    .flatten();
                let actor = self.actors.entry(a[3] & 0x0f).or_default();
                actor.layer.transition(duration, offset, false, mask);
                self.frame_dirty = true;
            }
            0x8b if a[3] & 0xfff0 == 0x10 && signed[0] >= 0 => {
                let actor = self.actors.entry(a[3] & 0x0f).or_default();
                actor.layer.scale = u32::from(a[0]);
                actor.layer.pivot = (signed[1], signed[2]);
                self.frame_dirty |= actor.layer.visible;
            }
            0x8b if a[3] & 0x20 != 0 => {
                self.actors.entry(a[3] & 0x0f).or_default().layer.order =
                    signed[0].clamp(0, 100) as u8;
                self.frame_dirty = true;
            }
            0x8c if a[3] == 0 => {
                self.cut_in.position = (signed[0], signed[1]);
                self.frame_dirty = true;
            }
            0x8c if a[3] == 255 => {}
            0x8c if (a[3] as u8 as i8) < 0 => {
                let duration = if self.control_keys != 0 || self.skip_dialogue {
                    Duration::ZERO
                } else {
                    Duration::from_millis(200 * u64::from(a[1]))
                };
                self.cut_in.transition(
                    duration,
                    0,
                    true,
                    self.surfaces.get(&self.texture_slots.mask).cloned(),
                );
                self.frame_dirty = true;
            }
            0x8c if a[3] & 0x40 != 0 && a[0] <= 3 => {
                let (frames, offset) = match a[0] {
                    0 => (6, -20),
                    1 => (6, 20),
                    2 => (20, 0),
                    _ => (12 * u64::from(a[1]), 0),
                };
                let duration = if self.control_keys != 0 || self.skip_dialogue {
                    Duration::ZERO
                } else {
                    Duration::from_micros(frames * 1_000_000 / 60)
                };
                let mask = (a[0] == 3)
                    .then(|| self.surfaces.get(&self.texture_slots.mask).cloned())
                    .flatten();
                self.cut_in.transition(duration, offset, false, mask);
                self.frame_dirty = true;
            }
            0x8c if a[3] & 0x10 != 0 => {
                self.cut_in.scale = u32::from(a[0]);
                self.cut_in.pivot = (signed[1], signed[2]);
                self.frame_dirty = true;
            }
            0x8c if a[3] & 0x20 != 0 => {
                self.cut_in.order = signed[0].clamp(0, 100) as u8;
                self.frame_dirty = true;
            }
            0x8d => {
                if let Some(sprite) = self.overlays.last_mut() {
                    sprite.x = signed[0];
                    sprite.y = signed[1];
                    self.frame_dirty = true;
                }
            }
            0x8e if a[3] & 0x30 == 0 => self.move_actor(a[2], (signed[0], signed[1]), a[3]),
            0x8e if a[3] & 0x20 != 0 => {
                self.actor_wave_frames = if a[0] == 0 { 60 } else { a[0] };
                self.actor_wave_vertical = a[1] != 0;
            }
            0x8e if a[3] & 0x10 != 0 => {
                if self.control_keys == 0 && !self.skip_dialogue && a[1] != a[2] {
                    let phase = if self.actor_wave_vertical { 90.0 } else { 0.0 };
                    let actor = self.actors.entry(a[3] & 0x0f).or_default();
                    actor.wave = Some(Wave {
                        elapsed: Duration::ZERO,
                        duration: Duration::from_micros(
                            u64::from(self.actor_wave_frames) * 1_000_000 / 60,
                        ),
                        amplitude: f64::from(signed[0]),
                        angles: (f64::from(signed[1]) + phase, f64::from(signed[2]) + phase),
                        vertical: self.actor_wave_vertical,
                    });
                    self.frame_dirty |= actor.layer.visible;
                }
            }
            0x90 if a[0] < 64 => {
                self.palette
                    .insert(a[0], [a[1] as u8, a[2] as u8, a[3] as u8]);
            }
            // This reserved token falls through the native dispatcher.
            0x91 => {}
            0x95 if self.script.dialect.version == 0x9f9a => {
                self.particle_region.bounds = signed;
                if let Some(falling) = &mut self.falling {
                    falling.set_region(self.particle_region);
                }
            }
            0x96 if self.script.dialect.version == 0x9f9a => {
                self.particle_region.weight = a[0];
                if let Some(falling) = &mut self.falling {
                    falling.set_region(self.particle_region);
                }
            }
            0x95 if a[3] == 4 => {
                if let Some(source) = self.animation_sources.get(&33).cloned() {
                    self.animation = Some(Animation::new(source, a[2], false)?);
                    self.animation_position = (signed[0], signed[1]);
                    self.frame_dirty = true;
                }
            }
            0x97 if self.script.dialect.version == 0x9f9a => {
                self.add_classic_route_choices(a[0] != 0)?
            }
            crate::mes::CLASSIC_SCENE_TRANSLATION if self.script.dialect.classic() => {
                self.translate_scene(a[0], (signed[2], signed[3]))?
            }
            0x99 => self.camera_duration = a[0],
            0x9d => self.position_camera(a)?,
            0x9e => self.texture_variant = a[0],
            0x9f | 0xa0 => {
                self.pan_background_plane(a[0], usize::from(code == 0xa0), signed[1], a[2])?
            }
            0xa2 => {
                if a[0] == 0 {
                    self.lens_position.center = (signed[1], signed[2]);
                } else {
                    self.lens_position.angles = (signed[1], signed[2]);
                    self.lens_position.duration = if signed[2] == -1 {
                        Duration::ZERO
                    } else {
                        Duration::from_millis(200 * u64::from(a[3]))
                    };
                }
            }
            0xa7 => {
                self.recording_motion = a[0] == 0;
                if self.recording_motion {
                    self.recorded_motion.clear();
                }
            }
            0xa8 if a[0] != 0 => {
                let was_recording = std::mem::replace(&mut self.recording_motion, false);
                let motion = self.recorded_motion.clone();
                let result = (|| {
                    for (script, offset) in motion {
                        let instruction = script.instruction(offset)?;
                        let code = script
                            .dialect
                            .opcode(instruction.opcode)
                            .ok_or_else(|| self.unsupported(instruction))?;
                        let Operands::Words(parameters) = instruction.args else {
                            return Err(Error::Invalid("ADV32 recorded layer motion"));
                        };
                        self.words(instruction, code, parameters, None)?;
                    }
                    Ok(())
                })();
                self.recording_motion = was_recording;
                result?;
            }
            0xa8 => {}
            0xa9 if a[3] <= 2 => {
                let mut percent = self.variable(99, 187)?.clamp(0, 99) as u8;
                if a[3] == 0 {
                    percent = percent.saturating_add(1).min(99);
                    self.set_variable(99, 187, i32::from(percent))?;
                }
                let channel = [0, 2, 1][usize::from(a[3])];
                if percent != 0
                    && let Some(audio) = &mut self.audio
                {
                    audio.seek_percent(channel, percent)?;
                }
            }
            0xa9 if matches!(a[3], 10 | 11) => {
                if let Some(audio) = &mut self.audio {
                    audio.pause_channel(1, a[3] == 11)?;
                }
            }
            0xab => self.control_comments(a)?,
            0xae if a[3] == 255 => self.actor_anchor = Some((a[0], signed[1], signed[2])),
            0xaf if a[3] < 3 && a[1] == 0 => {
                self.set_sound_pan(a[3] as usize, signed[0] as f32 / 100.0)?;
            }
            0xb0 if a[0] == 255 => {
                self.message_macro_slot = usize::from(a[1]).min(MESSAGE_MACROS - 1)
            }
            0xb0 => self.capture_message_macros()?,
            // Mode zero restores the normal source-over compositor.
            0xb1 => {
                if a[0] <= 100 {
                    self.mosaic = u32::from(a[0]) * 10;
                    self.mosaic_tween = None;
                    self.frame_dirty = true;
                }
            }
            0xb2 if a[0] <= 100 && a[1] <= 100 => {
                let (from, to) = (10 * i32::from(a[0]), 10 * i32::from(a[1]));
                self.mosaic = from as u32;
                let total = if self.control_keys != 0 || self.skip_dialogue {
                    Duration::ZERO
                } else {
                    Duration::from_micros(u64::from(a[2]) * 1_000_000 / 60)
                };
                self.mosaic_tween = (!total.is_zero()).then_some(Pan {
                    from: (from, 0),
                    to: (to, 0),
                    remaining: total,
                    total,
                });
                if total.is_zero() {
                    self.mosaic = to as u32;
                }
                self.frame_dirty = true;
            }
            0xb3 if a[0] == 0 => {}
            0xb4 => self.fit_current_background(self.extent)?,
            _ => return Err(self.unsupported(instruction)),
        }
        Ok(())
    }

    fn control_comments(&mut self, [command, value, end, duration]: [u16; 4]) -> Result<()> {
        if command == 99 {
            let asset = match value {
                1 => Some("SYSTEM\\COMMENTBACK"),
                2 => Some("SYSTEM\\COMUCHIP"),
                _ => None,
            };
            let background = if let Some(asset) = asset {
                match self.graphics.load(&self.game, asset) {
                    Ok(texture) => Some(texture),
                    Err(Error::MissingAsset(_)) => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            self.comments = if value == 0 {
                None
            } else {
                let (stamps, senders) = if value == 2 {
                    let stamps = match self.graphics.load(&self.game, "SYSTEM\\COMUSTAMP") {
                        Ok(texture) => Some(texture),
                        Err(Error::MissingAsset(_)) => None,
                        Err(error) => return Err(error),
                    };
                    let root = self
                        .game
                        .data
                        .parent()
                        .ok_or(Error::Invalid("ADV32 game directory"))?;
                    let senders = crate::native_data::comment_senders(root, self.text_encoding)?
                        .unwrap_or_default();
                    (stamps, senders)
                } else {
                    (None, BTreeMap::new())
                };
                Some(Comments::new(value, background, stamps, senders)?)
            };
        } else if let Some(comments) = &mut self.comments {
            match command {
                0 => comments.finish_batch(),
                u16::MAX => comments.clear(),
                1 => comments.show(true),
                2 => comments.set_period(value),
                3 => comments.set_random_order(value != 0),
                4 => comments.prefill(&mut self.random),
                5 => comments.scale_period(value as i16, end as i16, duration),
                6 => comments.set_clear_every(value),
                7 => comments.show(false),
                98 => comments.hide(),
                _ => return Err(Error::Unsupported("ADV32 comment overlay action")),
            }
        }
        self.frame_dirty = true;
        Ok(())
    }

    fn capture_message_macros(&mut self) -> Result<()> {
        let script = self.script.clone();
        let mut start = self.ip;
        let mut slot = self.message_macro_slot;
        let mut length = 0;
        loop {
            let instruction = script.instruction(self.ip)?;
            self.ip = instruction.next;
            match (script.dialect.opcode(instruction.opcode), instruction.args) {
                (Some(0xb0), Operands::Words([selector, next_slot, ..])) => {
                    self.message_macros[slot] = Some(MessageMacro {
                        script: script.clone(),
                        start,
                        end: instruction.offset,
                    });
                    if selector != 255 {
                        self.message_macro_slot = slot;
                        return Ok(());
                    }
                    slot = usize::from(next_slot).min(MESSAGE_MACROS - 1);
                    start = instruction.next;
                    length = 0;
                }
                (Some(0), _) => return Err(Error::Invalid("unterminated ADV32 message macro")),
                (Some(0x77), _) => {}
                _ if length < 10 => length += 1,
                _ => return Err(Error::Invalid("ADV32 message macro command count")),
            }
        }
    }

    fn run_message_effects(&mut self) -> Result<()> {
        let commands = self.message.as_mut().map_or_else(Vec::new, Message::due);
        for command in commands {
            match command {
                MessageCommand::Script(index) => self.run_message_macro(index)?,
                MessageCommand::Music(255) => self.stop_music()?,
                MessageCommand::Music(index) => self.play_music_track(index, true)?,
                MessageCommand::Sound(index, repeat) => self.play_effect(index, repeat)?,
                MessageCommand::FadeMusic(duration) => {
                    if let Some(audio) = &mut self.audio {
                        audio.fade_channel(0, Duration::from_millis(u64::from(duration)))?;
                    }
                    self.background_music = None;
                }
                MessageCommand::SwapTexture { from, to } => self.transfer_texture(from, to),
                MessageCommand::Shake { actor, vertical } => self.shake(actor, vertical, true),
                MessageCommand::ActorFade { actor, hide } => {
                    let duration = if self.control_keys != 0 || self.skip_dialogue {
                        Duration::ZERO
                    } else {
                        Duration::from_secs(1)
                    };
                    let slot = actor + self.texture_slots.actor;
                    let layer = &mut self.actors.entry(u16::from(actor)).or_default().layer;
                    if !hide && layer.texture.is_none() {
                        layer.texture = self.surfaces.get(&slot).cloned();
                    }
                    if hide {
                        self.surface_names.remove(&slot);
                    }
                    layer.transition(
                        duration,
                        0,
                        hide,
                        self.surfaces.get(&self.texture_slots.mask).cloned(),
                    );
                    self.frame_dirty = true;
                }
            }
        }
        Ok(())
    }

    fn run_message_macro(&mut self, index: usize) -> Result<()> {
        let Some(block) = self.message_macros.get(index).and_then(Option::clone) else {
            return Ok(());
        };
        let script = std::mem::replace(&mut self.script, block.script.clone());
        let ip = std::mem::replace(&mut self.ip, block.start);
        let wait = std::mem::replace(&mut self.wait, Wait::Ready);
        let result = (|| {
            while self.ip < block.end {
                let previous = self.ip;
                self.step()?;
                if !Arc::ptr_eq(&self.script, &block.script)
                    || self.ip <= previous
                    || self.ip > block.end
                {
                    return Err(Error::Unsupported("ADV32 control flow in message macro"));
                }
                if self.wait != Wait::Ready {
                    break;
                }
            }
            Ok(())
        })();
        self.script = script;
        self.ip = ip;
        self.wait = wait;
        result
    }

    fn picture_name(&self, bytes: &[u8]) -> Result<String> {
        let Some(number) = bytes.strip_prefix(b"*") else {
            return decode_japanese(bytes);
        };
        let number = std::str::from_utf8(number)
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or(Error::Invalid("ADV32 picture number"))?;
        self.sound_names.picture(number).map(str::to_owned)
    }

    fn load_surface(&mut self, slot: u8, bytes: &[u8]) -> Result<()> {
        if usize::from(slot) >= MAX_SURFACES {
            return Err(Error::Invalid("ADV32 surface number"));
        }
        let is_accessory = (self.texture_slots.accessory
            ..self.texture_slots.accessory + self.texture_slots.actor_count)
            .contains(&slot);
        if bytes.is_empty() || bytes == b"-" {
            self.panning_planes.remove(&slot);
            if is_accessory && let Some(name) = self.surface_names.get(&slot) {
                let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode(name);
                self.previous_accessories.insert(slot, encoded.into_owned());
            }
            self.surfaces.remove(&slot);
            self.surface_names.remove(&slot);
            self.animation_sources.remove(&slot);
        } else if bytes != b"--" {
            let mut name = self.picture_name(bytes)?;
            if name.to_ascii_lowercase().ends_with(".pck") {
                let source = Archive::open(self.game.resource("grp", &name, "pck")?)?;
                self.animation_sources.insert(slot, Arc::new(source));
                self.surfaces.remove(&slot);
            } else {
                let base = is_accessory
                    .then(|| {
                        let owner = slot - self.texture_slots.accessory + self.texture_slots.actor;
                        self.surface_names.get(&owner).map(String::as_str)
                    })
                    .flatten();
                let variant = if (4..=13).contains(&slot)
                    || ((19..=23).contains(&slot) && name.to_ascii_lowercase().starts_with("bst\\"))
                {
                    self.texture_variant
                } else {
                    0
                };
                let styled = graphics::variant_name(&name, variant, base);
                let texture = match self.graphics.load(&self.game, &styled) {
                    Ok(texture) => {
                        name = styled;
                        texture
                    }
                    Err(Error::MissingAsset(_)) if styled != name => {
                        self.graphics.load(&self.game, &name)?
                    }
                    Err(error) => return Err(error),
                };
                self.surfaces.insert(slot, texture);
                self.animation_sources.remove(&slot);
            }
            self.surface_names.insert(slot, name);
            if slot == 1 {
                self.animation = None;
                self.actors
                    .values_mut()
                    .for_each(|actor| actor.layer.visible = false);
            }
        }
        if slot == self.texture_slots.panel {
            self.message_panel = self
                .surfaces
                .get(&slot)
                .and_then(|image| message::panel(image, self.script.dialect));
        }
        Ok(())
    }

    fn save_slot(&mut self, slot: u16) -> Result<()> {
        if let Some(menu) = &self.script_menu {
            return self.saves.write_slot(slot, &menu.story, &menu.thumbnail);
        }
        let snapshot = self.snapshot()?;
        let thumbnail = self.compose()?;
        self.saves.write_slot(slot, &snapshot, &thumbnail)
    }

    fn snapshot(&self) -> Result<NativeSlot> {
        let mut layers = BTreeMap::new();
        if let Some(background) = &self.background {
            let mut position = (
                self.background_position.0 + background.x + self.scene_translation.0,
                self.background_position.1 + background.y + self.scene_translation.1,
            );
            let (width, height) = background.image.dimensions();
            if self.centered_background {
                position.0 += (self.extent.0 as i32 - width as i32) / 2 - background.x;
                position.1 += (self.extent.1 as i32 - height as i32) / 2 - background.y;
            }
            let extent = if let Some((source_width, source_height)) = self.fitted_background {
                position.0 += (width as i32 - source_width as i32) / 2;
                position.1 += (height as i32 - source_height as i32) / 2;
                (source_width, source_height)
            } else {
                (width, height)
            };
            layers.insert(
                0,
                NativeLayer {
                    position,
                    extent,
                    fitted_extent: self.fitted_background.map(|_| (width, height)),
                    pivot: self.camera.center,
                    scale: self.camera.scale,
                    visible: true,
                    order: 50,
                },
            );
        }
        for (&index, actor) in self.actors.iter().filter(|(id, _)| **id < 5) {
            let extent = actor
                .layer
                .texture
                .as_ref()
                .map_or((0, 0), |source| source.image.dimensions());
            layers.insert(
                10 * (usize::from(index) + 1),
                NativeLayer {
                    position: (
                        actor.layer.position.0 + self.scene_translation.0,
                        actor.layer.position.1 + self.scene_translation.1,
                    ),
                    extent,
                    fitted_extent: None,
                    pivot: actor.layer.pivot,
                    scale: actor.layer.scale,
                    visible: actor.layer.visible,
                    order: actor.layer.order,
                },
            );
        }
        if let Some(texture) = &self.cut_in.texture {
            layers.insert(
                63,
                NativeLayer {
                    position: self.cut_in.position,
                    extent: texture.image.dimensions(),
                    fitted_extent: None,
                    pivot: self.cut_in.pivot,
                    scale: self.cut_in.scale,
                    visible: self.cut_in.visible,
                    order: self.cut_in.order,
                },
            );
        }
        let (line, speaker) = self
            .message
            .as_ref()
            .map_or(("", ""), Message::native_parts);
        let music = self
            .background_music
            .map(|index| {
                self.sound_names
                    .music(index)
                    .map(|name| (index, name.to_owned()))
            })
            .transpose()?;
        Ok(NativeSlot {
            script: self.script_name.clone(),
            instruction: self.ip,
            resume: if self.wait == Wait::Input {
                self.message_offset
                    .or(self.resume_offset)
                    .unwrap_or(self.ip)
            } else {
                self.resume_offset.unwrap_or(self.ip)
            },
            entry: self.tick_label,
            entry_step: self.tick_step,
            assets: self.surface_names.clone(),
            music,
            speaker: if speaker.is_empty() {
                String::new()
            } else {
                format!("@s{speaker}")
            },
            line: line.to_owned(),
            variables: self.variables.clone(),
            layers,
            anchors: self.actor_anchors.clone(),
            texture_variant: self.texture_variant,
            preserve_view: self.preserve_view,
            fog_enabled: self.fog_enabled,
            player_names: self.player_names.clone(),
            custom_names: self.custom_names,
            lens_position: self.lens_position.clone(),
            lens_strength: self.lens.as_ref().map_or(0, |lens| lens.strength),
            comments: self
                .comments
                .as_ref()
                .map_or_else(Default::default, Comments::snapshot),
        })
    }

    fn load_slot(&mut self, slot: u16) -> Result<()> {
        let Some(snapshot) = self.saves.read_slot(slot)? else {
            return Ok(());
        };
        let instruction = snapshot.resume;
        self.script_menu = None;
        self.skip_dialogue = false;
        self.auto_dialogue = false;
        self.last_voice = None;
        self.restore_snapshot(snapshot, instruction)?;
        self.wait = Wait::Ready;
        Ok(())
    }

    fn restore_snapshot(&mut self, snapshot: NativeSlot, instruction: usize) -> Result<()> {
        let script = Arc::new(self.game.script(&snapshot.script)?);
        script.instruction(instruction)?;

        self.script = script;
        self.script_name = snapshot.script;
        self.classic_choice_text.clear();
        self.gallery_portrait = None;
        self.ip = instruction;
        self.resume_offset = Some(snapshot.resume);
        self.message_offset = self
            .script
            .instruction(snapshot.resume)
            .ok()
            .filter(|record| self.script.dialect.opcode(record.opcode) == Some(0x6b))
            .map(|record| record.offset);
        self.tick_label = snapshot.entry;
        self.tick_step = snapshot.entry_step;
        self.next_label = None;
        self.pending_voice = None;
        self.pending_log_entry = false;
        self.variables.retain(|&(bank, _), _| bank > 10);
        self.variables.extend(snapshot.variables);
        self.surface_names.clear();
        self.previous_accessories.clear();
        self.surfaces.clear();
        self.actors.clear();
        self.texture_variant = snapshot.texture_variant;
        self.preserve_view = snapshot.preserve_view;
        self.custom_names = snapshot.custom_names;
        if !snapshot.player_names.given.is_empty() {
            self.player_names.given = snapshot.player_names.given;
        }
        if !snapshot.player_names.family.is_empty() {
            self.player_names.family = snapshot.player_names.family;
        }
        self.fog_enabled = snapshot.fog_enabled;
        self.lens_position = snapshot.lens_position;
        self.set_lens(snapshot.lens_strength)?;
        self.falling = None;
        self.particle_region = ParticleRegion::default();
        if let Some(comments) = &mut self.comments {
            comments.restore(snapshot.comments, self.text_encoding, &mut self.random)?;
        }
        self.animation_sources.clear();
        self.animation = None;
        self.scene_shake = None;
        self.fade = None;
        self.mosaic = 0;
        self.mosaic_tween = None;
        self.transition = None;
        self.transition_backdrop = None;
        self.screen_break = None;
        self.panning_planes.clear();
        self.overlays.clear();
        self.overlay_text.clear();
        self.actor_anchors = snapshot.anchors;
        self.actor_anchor = None;
        for (index, name) in snapshot.assets {
            let (name, native_picture_pack) = name
                .strip_prefix("_p,")
                .map_or((name.as_str(), false), |name| (name, true));
            let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(name);
            if invalid {
                return Err(Error::Invalid("ADV32 save resource encoding"));
            }
            self.load_surface(if native_picture_pack { 33 } else { index }, &bytes)?;
        }
        self.background = self.surfaces.get(&1).cloned();
        self.background_position = (0, 0);
        self.centered_background = false;
        self.fitted_background = None;
        self.pan = None;
        self.camera_motion = None;
        self.scene_translation = (0, 0);
        self.scene_translation_motion = None;
        self.camera = Camera {
            scale: 100,
            center: (0, 0),
        };
        if let Some(layer) = snapshot.layers.get(&0) {
            self.background_position = layer.position;
            if let Some((width, height)) = layer.fitted_extent {
                self.fit_current_background((width, height))?;
                if let Some((source_width, source_height)) = self.fitted_background {
                    self.background_position.0 += (source_width as i32 - width as i32) / 2;
                    self.background_position.1 += (source_height as i32 - height as i32) / 2;
                }
            } else {
                self.camera = Camera {
                    scale: layer.scale.max(1),
                    center: layer.pivot,
                };
            }
            if let Some(background) = &self.background {
                self.background_position.0 -= background.x;
                self.background_position.1 -= background.y;
            }
        }
        for index in 0..self.texture_slots.actor_count {
            if let Some(layer) = snapshot.layers.get(&(10 * (usize::from(index) + 1))) {
                let source = self
                    .surfaces
                    .get(&(index + self.texture_slots.actor))
                    .cloned();
                self.actors.insert(
                    index as u16,
                    Actor {
                        layer: SceneLayer::restored(
                            source,
                            layer.position,
                            layer.pivot,
                            layer.scale,
                            layer.visible,
                            layer.order,
                        ),
                        ..Actor::default()
                    },
                );
            }
        }
        self.cut_in = SceneLayer::default();
        if let Some(layer) = snapshot.layers.get(&63) {
            self.cut_in.texture = self.surfaces.get(&self.texture_slots.cut_in).cloned();
            self.cut_in.position = layer.position;
            self.cut_in.pivot = layer.pivot;
            self.cut_in.scale = layer.scale;
            self.cut_in.visible = layer.visible && self.cut_in.texture.is_some();
            self.cut_in.order = layer.order;
        }
        self.message = (!snapshot.line.is_empty() || !snapshot.speaker.is_empty())
            .then(|| {
                Message::resumed(
                    snapshot.line,
                    snapshot.speaker,
                    &self.player_names,
                    self.custom_names,
                    self.script.dialect.version == 0x9f9a,
                )
            })
            .transpose()?;
        self.speaker.clear();
        self.stop_music()?;
        if let Some((index, name)) = snapshot.music {
            self.play_sound(&name, 0, true)?;
            self.background_music = Some(index);
        }
        self.frame_dirty = true;
        Ok(())
    }

    fn translate_scene(&mut self, mode: u16, (x, y): (i32, i32)) -> Result<()> {
        let to = match mode {
            0 | 1 => {
                // The signed coordinates are negated. The native -1 sentinel
                // centers that axis on the current background instead.
                let centered = self.background.as_ref().map_or((0, 0), |background| {
                    if self.centered_background {
                        return (-self.background_position.0, -self.background_position.1);
                    }
                    let (width, height) = background.image.dimensions();
                    (
                        self.extent.0 as i32 / 2
                            - width as i32 / 2
                            - self.background_position.0
                            - background.x,
                        self.extent.1 as i32 / 2
                            - height as i32 / 2
                            - self.background_position.1
                            - background.y,
                    )
                });
                (
                    if x == 1 { centered.0 } else { -x },
                    if y == 1 { centered.1 } else { -y },
                )
            }
            2 | 3 => (0, 0),
            _ => return Err(Error::Invalid("ADV32 scene translation mode")),
        };
        if matches!(mode, 0 | 2)
            || self.camera_duration == 0
            || self.control_keys != 0
            || self.skip_dialogue
            || self.scene_translation == to
        {
            self.scene_translation_motion = None;
            self.frame_dirty |= self.scene_translation != to;
            self.scene_translation = to;
        } else {
            let total = Duration::from_millis(u64::from(self.camera_duration) * 200);
            self.scene_translation_motion = Some(Pan {
                from: self.scene_translation,
                to,
                total,
                remaining: total,
            });
        }
        Ok(())
    }

    fn position_camera(&mut self, [mode, scale, x, y]: [u16; 4]) -> Result<()> {
        let (width, height) = self.extent;
        let half_x = width as i32 / 2 - 1;
        let half_y = height as i32 / 2 - 1;
        let to = Camera {
            scale: u32::from(scale),
            center: (
                i32::from(x as i16).clamp(-half_x, half_x),
                i32::from(y as i16).clamp(-half_y, half_y),
            ),
        };
        if scale == 0 {
            return Err(Error::Invalid("ADV32 camera scale"));
        }
        if mode == 0 || self.camera_duration == 0 {
            self.camera = to;
            self.camera_motion = None;
        } else {
            let total =
                Duration::from_micros(12 * u64::from(self.camera_duration) * 1_000_000 / 60);
            self.camera_motion = Some(CameraMotion {
                from: self.camera,
                to,
                total,
                remaining: total,
            });
        }
        self.frame_dirty = true;
        Ok(())
    }

    fn pan_background_plane(
        &mut self,
        index: u16,
        axis: usize,
        target: i32,
        ticks: u16,
    ) -> Result<()> {
        let slot = usize::from(self.texture_slots.plane) + usize::from(index);
        if slot >= MAX_SURFACES {
            return Err(Error::Invalid("ADV32 background plane number"));
        }
        let plane = self.panning_planes.entry(slot as u8).or_default();
        if ticks == 0 || self.control_keys != 0 || self.skip_dialogue {
            plane.position[axis] = target;
            plane.motions[axis] = None;
        } else {
            let total = Duration::from_millis(200 * u64::from(ticks));
            plane.motions[axis] = Some(Pan {
                from: (plane.position[axis], 0),
                to: (target, 0),
                total,
                remaining: total,
            });
        }
        self.frame_dirty = true;
        Ok(())
    }

    fn fit_current_background(&mut self, (width, height): (u32, u32)) -> Result<()> {
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > crate::crx::MAX_PIXELS as u64
        {
            return Err(Error::Invalid("ADV32 fitted background dimensions"));
        }
        if let Some(background) = &self.background {
            self.fitted_background
                .get_or_insert(background.image.dimensions());
            if background.image.dimensions() != (width, height) {
                self.background = Some(Arc::new(Crx::from_image(
                    0,
                    0,
                    image::imageops::resize(
                        background.image.as_ref(),
                        width,
                        height,
                        image::imageops::FilterType::Triangle,
                    ),
                )));
                self.frame_dirty = true;
            }
        }
        Ok(())
    }

    fn show_actor(&mut self, index: u8, name: &[u8]) -> Result<()> {
        let prepared = match name {
            b"." => Some(self.surface(index + self.texture_slots.actor)?),
            b"-" | b"--" => None,
            _ => return Err(Error::Unsupported("ADV32 actor resource command")),
        };
        let actor = self.actors.entry(u16::from(index)).or_default();
        if let Some(prepared) = prepared {
            actor.layer.texture = Some(prepared);
        }
        actor.layer.visible = name != b"-" && actor.layer.texture.is_some();
        self.frame_dirty = true;
        Ok(())
    }

    fn transfer_texture(&mut self, from: u8, to: u8) {
        let Some(texture) = self.surfaces.remove(&from) else {
            return;
        };
        if let Some(name) = self.surface_names.remove(&from) {
            self.surface_names.insert(to, name);
        }
        self.surfaces.insert(to, texture.clone());
        match to {
            1 => {
                self.background = Some(texture);
                self.fitted_background = None;
            }
            slot if (self.texture_slots.actor
                ..self.texture_slots.actor + self.texture_slots.actor_count)
                .contains(&slot) =>
            {
                self.actors
                    .entry(u16::from(slot - self.texture_slots.actor))
                    .or_default()
                    .layer
                    .texture = Some(texture)
            }
            _ => {}
        }
        self.frame_dirty = true;
    }

    fn shake(&mut self, actor: Option<u8>, vertical: bool, alternate: bool) {
        if self.control_keys != 0 || self.skip_dialogue {
            return;
        }
        let shake = Some(Shake {
            elapsed: Duration::ZERO,
            vertical,
            alternate,
        });
        if let Some(actor) = actor {
            self.actors.entry(u16::from(actor)).or_default().shake = shake;
        } else {
            self.scene_shake = shake;
        }
        self.frame_dirty = true;
    }

    fn move_actor(&mut self, target_actor: u16, mut target: (i32, i32), duration: u16) {
        let relative = target_actor & 0x40 != 0;
        let actor = self
            .actors
            .entry(if relative {
                target_actor & 15
            } else {
                target_actor
            })
            .or_default();
        if relative {
            let origin = *actor.relative_origin.get_or_insert(actor.layer.position);
            target.0 += origin.0;
            target.1 += origin.1;
        } else {
            actor.relative_origin = None;
        }
        let frames = if duration == 0 {
            60
        } else {
            u64::from(duration) * 6
        };
        let total = Duration::from_micros(frames * 1_000_000 / 60);
        let last = actor.next_motion.back().or(actor.motion.as_ref());
        let motion = Pan {
            from: last.map_or(actor.layer.position, |motion| motion.to),
            to: target,
            total,
            remaining: total,
        };
        if actor.motion.is_some() {
            actor.next_motion.push_back(motion);
        } else {
            actor.motion = Some(motion);
        }
        self.frame_dirty |= actor.layer.visible;
    }

    fn finish_scene_motion(&mut self) -> Result<()> {
        for plane in self.panning_planes.values_mut() {
            for (position, pan) in plane.position.iter_mut().zip(&mut plane.motions) {
                if let Some(motion) = pan.take() {
                    *position = motion.to.0;
                }
            }
        }
        if let Some(pan) = self.pan.take() {
            self.background_position = pan.to;
        }
        if let Some(tween) = self.mosaic_tween.take() {
            self.mosaic = tween.to.0 as u32;
        }
        if let Some(motion) = self.camera_motion.take() {
            self.camera = motion.to;
        }
        if let Some(pan) = self.scene_translation_motion.take() {
            self.scene_translation = pan.to;
        }
        if let Some(pan) = self.menu_pan.take() {
            self.active_menu_origin = pan.to;
        }
        for actor in self.actors.values_mut() {
            actor.shake = None;
            actor.wave = None;
            actor.layer.finish();
            if let Some(pan) = actor.next_motion.pop_back().or_else(|| actor.motion.take()) {
                actor.layer.position = pan.to;
            }
            actor.motion = None;
            actor.next_motion.clear();
        }
        self.scene_shake = None;
        self.transition = None;
        self.screen_break = None;
        self.cut_in.finish();
        for sprite in &mut self.overlays {
            if matches!(sprite.effect, Some(SpriteEffect::Entrance { .. })) {
                sprite.effect = None;
            }
        }
        if let Some(animation) = &mut self.animation {
            animation.complete()?;
        }
        self.fade = None;
        self.frame_dirty = true;
        Ok(())
    }

    fn set_lens(&mut self, strength: u8) -> Result<()> {
        if strength == 0 {
            self.lens = None;
        } else {
            let resource = if self.script.dialect.version == 0x9e97 {
                "SYSTEM\\LENS"
            } else {
                "SYSTEM\\EFFLENS"
            };
            let texture = self.graphics.load(&self.game, resource)?;
            self.lens = Some(LensFlare::new(texture, strength, &mut self.random));
        }
        self.frame_dirty = true;
        Ok(())
    }

    fn open_menu(&mut self, group: u8, variant: Option<u16>) {
        if let Some(template) = self.templates.get(&group) {
            self.overlays.clone_from(&template.sprites);
            self.overlay_text.clone_from(&template.text);
        }
        if self
            .parties
            .get(&group)
            .is_some_and(|buttons| !buttons.is_empty())
        {
            self.button_group = group;
            self.menu_variant = variant;
            self.confirmation = None;
            self.wait = Wait::Menu;
            self.active_menu_origin = (self.menu_position.0, self.menu_position.1);
            self.keyboard_selection = None;
            self.dragged_button = None;
            self.frame_dirty = true;
        }
    }

    fn open_confirmation(&mut self, group: u8, confirmation: Confirmation) {
        self.open_menu(group, Some(self.menu_position.2));
        if self.wait == Wait::Menu && self.button_group == group {
            self.confirmation = Some(confirmation);
        }
    }

    fn save_grid_slots(&self) -> Result<std::ops::Range<u16>> {
        let count = self.slot_rectangles.len().min(10) as u16;
        let page = self.variable(99, 0)?.clamp(0, 9) as u16;
        let first = match self.script.dialect.version {
            0x9f9a if self.variable(3, 20)? != 0 => 100,
            0x9f9a => count * page,
            _ => 3 + count * page,
        };
        Ok(first..first + count)
    }

    fn queue_save_grid(&mut self, loading: bool) -> Result<()> {
        if self.slot_rectangles.is_empty() || self.script.label(b"selsave", 0x4b)?.is_none() {
            return Ok(());
        }
        let slots = self.save_grid_slots()?;
        let classic = self.script.dialect.version == 0x9f9a;
        let wide = classic && self.variable(3, 20)? != 0;
        let atlas = self.surfaces.get(&if classic { 14 } else { 19 });
        let buttons = self.parties.entry(self.button_group).or_default();
        buttons.retain(|button| button.save_slot.is_none());
        for (slot, &area) in slots.zip(&self.slot_rectangles) {
            let caption = self.saves.slot_caption(slot, wide)?;
            let occupied = caption.is_some();
            let selectable = if loading {
                occupied
            } else {
                !classic || slot >= 3
            };
            let mut states = BTreeMap::new();
            let sprite = |source: &Arc<Crx>, (x, y), (sx, sy, width, height)| Sprite {
                source: source.clone(),
                x: area[0] + x,
                y: area[1] + y,
                clip: Some(Clip {
                    x: sx,
                    y: sy,
                    width,
                    height,
                }),
                use_window_alpha: false,
                effect: None,
            };
            if let Some(atlas) = atlas {
                for state in [99, 1, 2] {
                    let idle = state == 99;
                    let clip = if wide {
                        (0, if !idle && selectable { 92 } else { 0 }, 659, 91)
                    } else if classic {
                        (
                            if !idle && selectable { 532 } else { 190 },
                            185 + 89 * i32::from(slot.min(3)),
                            341,
                            88,
                        )
                    } else {
                        (2, 21, 447, 105)
                    };
                    let mut images = vec![sprite(atlas, (0, 0), clip)];
                    if !classic {
                        let outline_x = if !idle && selectable { 148 } else { 16 };
                        images.push(sprite(atlas, (14, 14), (outline_x, 127, 129, 75)));
                        if let Some(source) = self.slot_thumbnails.get(&slot) {
                            images.push(sprite(source, (16, 16), (0, 0, 125, 71)));
                        }
                    }
                    states.insert(state, images);
                }
            }
            let (date, annotation) =
                caption.unwrap_or_else(|| ("---- --/-- --:--:--".into(), "No Data".into()));
            let (x, date_y, title_y) = if classic { (48, 16, 40) } else { (160, 12, 50) };
            let date_size = if classic || !occupied { 20 } else { 18 };
            let title_size = if classic || !occupied {
                20
            } else {
                // The native small-font selectors depend on length in MES bytes.
                match self.text_encoding.encode(&annotation).0.len() {
                    33.. => 12,
                    29..=32 => 16,
                    _ => 18,
                }
            };
            let width = Some((area[2] - area[0] - x - 8).max(1) as u32);
            let text = |content, y, size| Text {
                fixed_pitch: true,
                line_height: None,
                outline: true,
                clip_width: None,
                content,
                x: area[0] + x,
                y: area[1] + y,
                size,
                color: [255; 3],
                width,
            };
            buttons.push(Button {
                area,
                label: if selectable {
                    b"selsave".to_vec()
                } else {
                    Vec::new()
                },
                states,
                captions: BTreeMap::from([(
                    99,
                    vec![
                        text(date, date_y, date_size),
                        text(annotation, title_y, title_size),
                    ],
                )]),
                save_slot: Some(slot),
                ..Button::default()
            });
        }
        self.frame_dirty = true;
        Ok(())
    }

    fn fade_in(&mut self, preset: u8) -> Result<()> {
        if self.variable(99, 10)? == 0 {
            self.fade = None;
            return Ok(());
        }
        let nominal = 20 * (u64::from(preset) + 1);
        let speed = self.variable(99, 12)?.clamp(0, 5) as u64;
        let samples = nominal + (nominal / 2) * (5 - speed) / 5;
        let total = Duration::from_micros(samples * 1_000_000 / 60);
        self.fade = Some(Fade {
            total,
            remaining: total,
            color: self.fade_color,
            mask: None,
            pulse: false,
        });
        self.frame_dirty |= self.background.is_some();
        Ok(())
    }

    fn mask_fade(&mut self, duration: u8, increase: bool) -> Result<()> {
        if self.variable(99, 10)? == 0 || duration == 0 {
            return Ok(());
        }
        let total = Duration::from_millis(u64::from(duration) * 200);
        self.fade = Some(Fade {
            total,
            remaining: total,
            color: self.fade_color,
            mask: Some((self.surface(self.texture_slots.mask)?, increase)),
            pulse: false,
        });
        self.frame_dirty = true;
        Ok(())
    }

    fn pan_to(&mut self, to: (i32, i32), mode: u16, count: u16) {
        if mode == 1 {
            self.pan = None;
            self.background_position = to;
            self.frame_dirty = true;
            return;
        }
        let samples = match mode {
            0 => 120,
            99 => 12 * u64::from(count),
            _ => 15 * (u64::from(count) + 4),
        };
        if samples == 0 {
            self.background_position = to;
        } else {
            let total = Duration::from_micros(samples * 1_000_000 / 60);
            self.pan = Some(Pan {
                from: self.background_position,
                to,
                total,
                remaining: total,
            });
            self.wait = Wait::Timer {
                remaining: total,
                skippable: true,
            };
        }
        self.frame_dirty = true;
    }

    fn caption_style(&self, mut content: String) -> (String, u16) {
        let normal = if self.script.dialect.classic() {
            24
        } else {
            22
        };
        let size = match content.as_bytes().get(..2) {
            Some(b"@l") => 32,
            Some(b"@s") => 20,
            Some(b"@i") => 18,
            Some(b"@y") => 16,
            Some(b"@r") => 12,
            Some(b"@z") => 10,
            Some(b"@m" | b"@w") => normal,
            _ => return (content, normal),
        };
        content.drain(..2);
        (content, size)
    }

    fn queue_overlay_text(&mut self, [x, y, size, palette]: [u16; 4]) -> Result<()> {
        let operand = self.script.instruction(self.ip)?;
        let Operands::Text { bytes, .. } = operand.args else {
            return Err(Error::Invalid("ADV32 overlay text operand"));
        };
        let (content, font_size) =
            self.caption_style(Script::decode_text(bytes, self.text_encoding)?);
        self.ip = operand.next;
        let outline = palette > 128;
        let palette = palette & 0x7f;
        let color = self
            .palette
            .get(&(palette + 12))
            .copied()
            .unwrap_or([255; 3]);
        self.overlay_text.push(Text {
            fixed_pitch: true,
            line_height: (size < 99).then_some(size),
            outline,
            clip_width: None,
            content,
            x: i32::from(x as i16),
            y: i32::from(y as i16),
            size: font_size,
            color,
            width: None,
        });
        self.frame_dirty = true;
        Ok(())
    }

    fn play_sound(&mut self, name: &str, channel: u8, repeat: bool) -> Result<()> {
        let gain = self.sound_gain(channel);
        let bus = match channel {
            0 => 0,
            1 => 2,
            _ => 1,
        };
        let filename = name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(name)
            .to_ascii_uppercase();
        let pan = if channel == 1 {
            self.sound_pan_prefixes
                .iter()
                .find(|(prefix, _)| filename.starts_with(prefix))
                .map_or(self.sound_pan[bus], |(_, pan)| *pan)
        } else {
            self.sound_pan[bus]
        };
        self.ensure_audio()?;
        self.audio
            .as_mut()
            .expect("audio was opened")
            .load_and_play(&self.game, name, channel, repeat, gain, pan)
    }

    fn play_effect(&mut self, index: u16, repeat: bool) -> Result<()> {
        let name = self.sound_names.effect(index)?.to_owned();
        let channel = self.next_effect_channel;
        match self.play_sound(&name, channel, repeat) {
            // An entry in the engine's table can have no effect asset in a release.
            Ok(()) => self.next_effect_channel = if channel == 5 { 2 } else { channel + 1 },
            Err(Error::MissingAsset(_)) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }

    fn play_button_sound(&mut self, index: u16) -> Result<()> {
        if index == 0 || self.sound_gain(BUTTON_SOUND_CHANNEL) == 0.0 {
            return Ok(());
        }
        let Ok(name) = self.sound_names.effect(index).map(str::to_owned) else {
            return Ok(());
        };
        match self.play_sound(&name, BUTTON_SOUND_CHANNEL, false) {
            Err(Error::MissingAsset(_)) => Ok(()),
            result => result,
        }
    }

    #[cfg(feature = "video-ffmpeg")]
    fn start_movie(&mut self, name: &str) -> Result<()> {
        let path = self.game.resource("movie", name, "mpg")?.to_path_buf();
        self.ensure_audio()?;
        self.stop_music()?;
        for channel in 1..=BUTTON_SOUND_CHANNEL {
            self.stop_sound(channel)?;
        }
        self.movie = Some(Movie::open(path)?);
        self.wait = Wait::Movie;
        self.frame_dirty = true;
        Ok(())
    }

    #[cfg(feature = "video-ffmpeg")]
    fn stop_movie(&mut self) -> Result<()> {
        if let Some(movie) = self.movie.take() {
            let audio = self
                .audio
                .as_ref()
                .ok_or(Error::Invalid("ADV32 movie audio device"))?;
            movie.close(audio)?;
        }
        self.wait = Wait::Ready;
        self.frame_dirty = true;
        Ok(())
    }

    fn set_sound_pan(&mut self, bus: usize, pan: f32) -> Result<()> {
        let pan = pan.clamp(-1.0, 1.0);
        self.sound_pan[bus] = pan;
        let channels: &[u8] = match bus {
            0 => &[0],
            1 => &[2, 3, 4, 5],
            _ => &[1],
        };
        if let Some(audio) = &mut self.audio {
            for &channel in channels {
                audio.set_pan(channel, pan)?;
            }
        }
        Ok(())
    }

    fn sound_gain(&self, channel: u8) -> f32 {
        let (enabled, volume) = match channel {
            0 => (14, 7),
            1 => (5, 9),
            _ => (15, 8),
        };
        let setting = |index, default| {
            self.variables
                .get(&(99, index))
                .copied()
                .or_else(|| self.saves.global(99, index))
                .unwrap_or(default)
        };
        let gain = if channel == 0
            && let Some(volume) = self.music_volume
        {
            volume
        } else {
            setting(volume, 100).clamp(0, 100) as f32 / 100.0
        };
        if setting(enabled, 1) == 0 { 0.0 } else { gain }
    }

    fn control_music_volume(&mut self, value: u8) -> Result<()> {
        match value {
            254 => return Ok(()),
            255 if self.music_volume.is_some() => {
                self.music_volume = self.saved_music_volume.take()
            }
            255 => return Ok(()),
            _ => {
                let base = self
                    .music_volume
                    .unwrap_or(self.variable(99, 7)?.clamp(0, 100) as f32 / 100.0);
                self.saved_music_volume = self.music_volume;
                self.music_volume = Some((base * f32::from(value) / 100.0).min(1.0));
            }
        }
        let gain = self.sound_gain(0);
        if let Some(audio) = &mut self.audio {
            audio.set_volume(0, gain)?;
        }
        Ok(())
    }

    fn stop_sound(&mut self, channel: u8) -> Result<()> {
        if let Some(audio) = &mut self.audio {
            audio.stop_channel(channel)?;
        }
        Ok(())
    }

    fn stop_music(&mut self) -> Result<()> {
        self.background_music = None;
        self.stop_sound(0)
    }

    fn start_staff_roll(&mut self, route_slides: bool) -> Result<()> {
        let roll = if self.script.dialect.version == 0x9f9a {
            if route_slides {
                let route = self.variable(5, 10)? as u16;
                let root = self
                    .game
                    .data
                    .parent()
                    .ok_or(Error::Invalid("ADV32 game directory"))?;
                let slides = credit_slides(root, route)?
                    .ok_or(Error::Unsupported("original ADV32 staff roll archive"))?;
                let stem = slides
                    .strip_suffix(".pck")
                    .ok_or(Error::Invalid("ADV32 staff roll archive name"))?;
                let list = self.game.resource("grp", &format!("{stem}list"), "pck")?;
                let pictures = self.game.resource("grp", &slides, "pck")?;
                let duration = Duration::from_secs(if route == 4 { 117 } else { 176 });
                let backdrop = self.graphics.load(&self.game, r"ED\edcharbase")?;
                StaffRoll::classic(
                    Archive::open(list)?,
                    duration,
                    backdrop,
                    Some(Archive::open(pictures)?),
                )?
            } else {
                let path = self.game.resource("grp", r"ED\ed", "pck")?;
                let backdrop = self.graphics.load(&self.game, r"ED\gedback01")?;
                let mut roll = StaffRoll::classic(
                    Archive::open(path)?,
                    Duration::from_secs(289),
                    backdrop,
                    None,
                )?;
                roll.set_followup_backdrops([
                    self.graphics.load(&self.game, r"ED\gedback02")?,
                    self.graphics.load(&self.game, r"ED\gedback03")?,
                ]);
                roll
            }
        } else {
            let path = self.game.resource("grp", r"ED\ed", "pck")?;
            let duration = Duration::from_secs(self.variable(3, 0)?.max(0) as u64);
            StaffRoll::new(Archive::open(path)?, duration)?
        };
        self.staff_roll = Some(roll);
        self.frame_dirty = true;
        self.wait = Wait::Credits;
        Ok(())
    }

    fn refresh_saved_preferences(&mut self) -> Result<()> {
        self.variables
            .retain(|&(bank, index), _| !self.saves.is_global(bank, index));
        for channel in 0..=BUTTON_SOUND_CHANNEL {
            let gain = self.sound_gain(channel);
            if let Some(audio) = &mut self.audio {
                audio.set_volume(channel, gain)?;
            }
        }
        Ok(())
    }

    fn play_music_track(&mut self, track: u16, repeat: bool) -> Result<()> {
        let name = self.sound_names.music(track)?.to_owned();
        self.play_sound(&name, 0, repeat)?;
        self.background_music = Some(track);
        self.set_variable(96, track, 1)
    }

    fn load_music_tracks(&mut self) -> Result<()> {
        if self.music_tracks.is_empty() {
            let root = self
                .game
                .data
                .parent()
                .ok_or(Error::Invalid("ADV32 game directory"))?;
            let playlist = music_playlist(root, self.text_encoding)?
                .ok_or(Error::Unsupported("original ADV32 music playlist"))?;
            (self.music_tracks, self.music_titles) = playlist
                .into_iter()
                .map(|track| (track.id, track.title))
                .unzip();
            self.music_order = self.music_tracks.clone();
        }
        Ok(())
    }

    fn music_room(&mut self, command: u8) -> Result<()> {
        self.load_music_tracks()?;
        let count = self.music_order.len();
        let cursor = usize::try_from(self.variable(7, 98)?)
            .ok()
            .filter(|&index| index < count)
            .unwrap_or(0);
        let favorites =
            || (0..MUSIC_TRACK_LIMIT as u16).filter(|&track| self.saves.music_favorite(track));
        let ((favorite_bank, favorite_index), (order_bank, order_index)) =
            if self.script.dialect.version == 0x9f9a {
                ((3, 29), (9, 30))
            } else {
                ((100, 21), (100, 26))
            };
        let following_favorites = self.variable(favorite_bank, favorite_index)? != 0;
        match command {
            0 => {
                let track = self.music_order[cursor];
                let has_favorites = favorites().next().is_some();
                if !has_favorites && following_favorites {
                    self.set_variable(favorite_bank, favorite_index, 0)?;
                }
                self.music_order.clone_from(&self.music_tracks);
                if self.variable(order_bank, order_index)? > 1 {
                    for end in (1..count).rev() {
                        let position = rand15(&mut self.random) as usize % (end + 1);
                        self.music_order.swap(end, position);
                    }
                }
                let position = self
                    .music_order
                    .iter()
                    .position(|&candidate| {
                        candidate == track
                            && (!following_favorites
                                || !has_favorites
                                || self.saves.music_favorite(candidate))
                    })
                    .or_else(|| {
                        self.music_order
                            .iter()
                            .position(|&candidate| self.saves.music_favorite(candidate))
                    })
                    .unwrap_or(0);
                self.set_variable(7, 98, position as i32)?;
                if following_favorites && has_favorites {
                    self.set_variable(99, 254, position as i32)?;
                }
            }
            1 | 99 => {
                let track = self.music_order[cursor];
                let index = self
                    .music_tracks
                    .iter()
                    .position(|&entry| entry == track)
                    .unwrap_or(0);
                self.set_variable(99, 254, index as i32)?;
                self.set_variable(3, 11, 0)?;
                if command == 1 {
                    self.play_music_track(track, false)?;
                } else {
                    let percent = self.variable(99, 187)?.clamp(0, 99) as u8;
                    if let Some(audio) = &mut self.audio {
                        audio.seek_percent(0, percent)?;
                    }
                }
            }
            2 | 3 => {
                let filter = following_favorites && favorites().next().is_some();
                let step = if command == 2 { 1 } else { count - 1 };
                let mut position = cursor;
                for _ in 0..count {
                    position = (position + step) % count;
                    if !filter || self.saves.music_favorite(self.music_order[position]) {
                        break;
                    }
                }
                self.set_variable(7, 98, position as i32)?;
            }
            5 => {
                let track = self.music_tracks[cursor];
                self.set_variable(3, 11, 0)?;
                if following_favorites {
                    self.saves.set_music_favorite(track, true);
                }
                self.play_music_track(track, false)?;
                if let Some(position) = self.music_order.iter().position(|&entry| entry == track) {
                    self.set_variable(7, 98, position as i32)?;
                }
            }
            6 => self.set_variable(3, 0, favorites().count() as i32)?,
            _ => {}
        }
        Ok(())
    }

    fn music_favorite(&mut self, update: bool, index: u8) -> Result<()> {
        if update && index >= 254 {
            for track in 0..MUSIC_TRACK_LIMIT as u16 {
                self.saves.set_music_favorite(track, index == 255);
            }
        } else {
            self.load_music_tracks()?;
            let offset = self.variable(99, 246)?.max(0) as usize;
            let track = self.music_tracks.get(offset + usize::from(index)).copied();
            let favorite = track.is_some_and(|track| self.saves.music_favorite(track));
            if update {
                if let Some(track) = track {
                    self.saves.set_music_favorite(track, !favorite);
                }
            } else {
                return self.set_variable(3, 0, i32::from(favorite));
            }
        }
        if self.script.dialect.classic() {
            Ok(())
        } else {
            self.saves.flush_globals()
        }
    }

    fn gallery_category(&self) -> Result<Option<(usize, usize)>> {
        let category = self.variable(3, 5)? as usize;
        if category >= 6 {
            return Ok(None);
        }
        let variant = if category == 5 {
            self.variable(3, 84)?.clamp(0, 3) as usize
        } else {
            0
        };
        Ok(Some((category + variant, variant)))
    }

    fn gallery(&mut self) -> Result<&GalleryProfile> {
        if self.gallery.is_none() {
            let parent = self
                .game
                .data
                .parent()
                .ok_or(Error::Invalid("ADV32 game directory"))?;
            self.gallery = gallery_profile(parent)?;
        }
        self.gallery
            .as_ref()
            .ok_or(Error::Unsupported("original ADV32 gallery profile"))
    }

    fn check_gallery_page(&mut self, control: u8) -> Result<()> {
        let Some((category, variant)) = self.gallery_category()? else {
            return self.set_variable(3, 0, 99);
        };
        let page = self.variable(3, 0)?;
        let outfit = self.variable(3, 80)?;
        let pose = self.variable(3, 81)?;
        let alternate = self.variable(3, 85)? != 0;
        let invalid = match control {
            0 | 1 => page >= i32::from(self.gallery()?.limits[category][usize::from(control)]),
            2 => {
                let has_alternate =
                    (category >= 5 && variant < 2) || (category == 0 && alternate && pose == 1);
                if has_alternate && pose >= 1 {
                    self.set_variable(3, 82, 0)?;
                }
                let first_pose = if has_alternate {
                    1
                } else if category < 5 {
                    2
                } else {
                    0
                };
                pose >= first_pose && page != 0
            }
            3 if (0..=2).contains(&pose) => {
                let column = if category == 5 && pose == 1 {
                    4
                } else {
                    pose as usize + 2
                };
                page >= i32::from(self.gallery()?.limits[category][column])
            }
            4 => category != 1 || outfit != 2,
            5 => category != 0 || outfit != 2 || !alternate,
            _ => false,
        };
        if invalid {
            self.set_variable(3, 0, 99)?;
        }
        Ok(())
    }

    fn show_gallery_portrait(&mut self) -> Result<()> {
        self.gallery_portrait = None;
        self.frame_dirty = true;
        let Some((category, variant)) = self.gallery_category()? else {
            return Ok(());
        };
        let outfit = self.variable(3, 80)?;
        let pose = self.variable(3, 81)?;
        let expression = self.variable(3, 82)?;
        let index = self.variable(3, 83)?.max(0) as usize;
        let alternate = self.variable(3, 85)? != 0;
        let combined = self.variable(3, 86)? != 0;
        if !(0..=3).contains(&outfit) || !(0..=2).contains(&pose) || !(0..=1).contains(&expression)
        {
            return Ok(());
        }
        let prefix = &self.gallery()?.prefixes[category];
        let costume = match outfit {
            0 if matches!(variant, 1 | 3) => 'b',
            2 if category == 0 && alternate && combined => 'z',
            _ => char::from(b'a' + outfit as u8),
        };
        let pose = if category >= 5 && pose == 1 {
            3
        } else {
            pose + 1
        };
        let expression = char::from(b'a' + expression as u8);
        let variant_suffix = if alternate { "n" } else { "" };
        let archive_name = format!(r"BST\{prefix}_{costume}");
        let prefix = format!("{prefix}_{costume}{pose}_{expression}{variant_suffix}");
        let number = if variant >= 2 {
            let archive = Archive::open(self.game.resource("grp", &archive_name, "crm")?)?;
            (1..=99)
                .filter(|value| {
                    archive
                        .find_name(&format!("{prefix}{value:02}.crx"))
                        .is_some()
                        || archive.find_name(&format!("{prefix}{value:02}")).is_some()
                })
                .nth(index)
        } else {
            Some(index + 1)
        };
        if let Some(number) = number {
            let resource = format!(r"BST\{prefix}{number:02},{archive_name}");
            self.gallery_portrait = match self.graphics.load(&self.game, &resource) {
                Ok(portrait) => Some(portrait),
                Err(Error::MissingAsset(_)) => None,
                Err(error) => return Err(error),
            };
        }
        Ok(())
    }

    fn surface(&self, slot: u8) -> Result<Arc<Crx>> {
        self.surfaces
            .get(&slot)
            .cloned()
            .ok_or(Error::MissingSurface(slot))
    }

    fn jump(&mut self, label: &[u8]) -> Result<()> {
        if let Some(offset) = self.script.label(label, 0x4b)? {
            self.ip = offset;
        }
        Ok(())
    }

    fn switch_script(&mut self, name: &[u8]) -> Result<()> {
        let name = decode_japanese(name)?;
        let script = Arc::new(self.game.script(&name)?);
        let label = self.next_label.take();
        let entry = match label {
            Some(label) => script.label(&label, 0x4b)?.unwrap_or(script.entry(0)?),
            None => script.entry(0)?,
        };
        self.script = script;
        self.script_name = name;
        self.resume_offset = None;
        self.message_offset = None;
        self.pending_voice = None;
        self.pending_log_entry = false;
        self.classic_choice_text.clear();
        self.gallery_portrait = None;
        self.ip = entry;
        self.music_end_route = None;
        self.scroll_list = None;
        self.slider_drag = None;
        self.dragged_button = None;
        self.mouse_gesture = None;
        self.quick_prompt = None;
        self.confirmation = None;
        self.message = None;
        self.speaker.clear();
        Ok(())
    }

    fn compare(&mut self, args: [u16; 4]) -> Result<()> {
        let actual = self.variable(args[0], args[1])?;
        let operation = args[3] & 0x7f;
        let expected = if args[3] & 0x80 != 0 {
            self.variable(args[2] >> 8, args[2] & 0xff)?
        } else {
            i32::from(args[2] as i16)
        };
        let passes = match operation {
            0 => actual == expected,
            1 => actual != expected,
            2 => actual > expected,
            3 => actual < expected,
            4 => actual >= expected,
            5 => actual <= expected,
            _ => return Err(Error::Unsupported("ADV32 comparison operation")),
        };
        let Instruction {
            opcode: 2,
            args: Operands::Pair([block, _]),
            next,
            ..
        } = self.script.instruction(self.ip)?
        else {
            return Err(Error::Invalid("ADV32 comparison block delimiter"));
        };
        self.ip = next;
        if !passes {
            self.skip_block(block)?;
        }
        Ok(())
    }

    fn skip_block(&mut self, block: u8) -> Result<()> {
        loop {
            let next = self.script.instruction(self.ip)?;
            self.ip = next.next;
            match (self.script.dialect.opcode(next.opcode), next.args) {
                (Some(0x20), Operands::Pair([_, number])) if number == block => return Ok(()),
                (Some(0), _) => {
                    return Err(Error::Invalid("unterminated ADV32 conditional block"));
                }
                _ => {}
            }
        }
    }

    fn variable(&self, bank: u16, index: u16) -> Result<i32> {
        if (bank, index) == (99, 4) {
            return Ok(i32::from(self.fullscreen));
        }
        if let Some(value) = self.variables.get(&(bank, index)) {
            return Ok(*value);
        }
        if (bank, index) == (99, 21) {
            return Ok(i32::from(self.saves.exists(2)));
        }
        if (bank, index) == (99, 250) {
            return self.variable(99, 27);
        }
        Ok(self.saves.global(bank, index).unwrap_or(0))
    }

    fn set_variable(&mut self, bank: u16, index: u16, value: i32) -> Result<()> {
        let value = if (bank <= 10 && index < 100) || (96..=98).contains(&bank) || bank == 100 {
            i32::from(value as u8)
        } else {
            value
        };
        let previous = self.variables.insert((bank, index), value);
        self.saves.update_global(bank, index, value);
        self.frame_dirty |= (bank, index) == (99, 11) && previous != Some(value);
        if bank == 99 && self.audio.is_some() {
            let channels: &[u8] = match index {
                7 | 14 => &[0],
                5 | 9 => &[1],
                8 | 15 => &[2, 3, 4, 5, BUTTON_SOUND_CHANNEL],
                _ => &[],
            };
            for &channel in channels {
                let gain = self.sound_gain(channel);
                if let Some(audio) = &mut self.audio {
                    audio.set_volume(channel, gain)?;
                }
            }
        }
        if bank == 100 && index <= 0x95 {
            if index == 49 {
                let previous = self.variables.insert((99, 11), value);
                self.saves.update_global(99, 11, value);
                self.frame_dirty |= previous != Some(value);
            }
            self.saves.flush_globals()?;
        }
        Ok(())
    }

    fn unsupported(&self, instruction: Instruction<'_>) -> Error {
        Error::UnsupportedCommand {
            opcode: instruction.opcode,
            script: self.script_name.clone(),
            offset: instruction.offset,
        }
    }
}

fn draw_buttons(
    frame: &mut Canvas,
    buttons: &[Button],
    hovered: Option<usize>,
    message_bar: bool,
    variant: Option<u16>,
    origin: (i32, i32),
    painter: &mut TextPainter,
    slider_drag: Option<SliderDrag>,
    slider_value: impl Fn(Slider) -> i32,
) -> Result<()> {
    let buttons = buttons.iter().enumerate();
    let ordered = buttons
        .clone()
        .filter(|(_, button)| !button.draw_on_top)
        .chain(buttons.filter(|(_, button)| button.draw_on_top));
    for (index, button) in ordered {
        let decoration = variant.filter(|_| button.label.is_empty());
        let states: &[u16] = if decoration.is_some() {
            &[99]
        } else if hovered == Some(index) {
            &[2, 1, 99]
        } else if message_bar {
            &[99]
        } else {
            &[99, 1]
        };
        let sprites = states.iter().find_map(|state| button.states.get(state));
        // An empty-label modal button carries its common artwork in state 99
        // and one header sprite for each prompt type in state 2.
        let header =
            decoration.and_then(|variant| button.states.get(&2)?.get(usize::from(variant)));
        for sprite in sprites.into_iter().flatten().chain(header) {
            graphics::blit(
                frame,
                &sprite.source,
                sprite.x + origin.0,
                sprite.y + origin.1,
                sprite.clip,
            );
        }
        if let Some(slider) = button.slider {
            let dragging = slider_drag.filter(|drag| drag.button == index);
            let thumb_state = if dragging.is_some() { 8 } else { 6 };
            let value = dragging.map_or_else(|| slider_value(slider), |drag| drag.value);
            for sprite in button.states.get(&thumb_state).into_iter().flatten() {
                graphics::blit(
                    frame,
                    &sprite.source,
                    slider.thumb_x(value) + origin.0,
                    sprite.y + origin.1,
                    sprite.clip,
                );
            }
            if dragging.is_some() || hovered == Some(index) {
                for sprite in button.states.get(&10).into_iter().flatten() {
                    graphics::blit(
                        frame,
                        &sprite.source,
                        sprite.x + origin.0,
                        sprite.y + origin.1,
                        sprite.clip,
                    );
                }
            }
        }
        if let Some(captions) = states.iter().find_map(|state| button.captions.get(state)) {
            for caption in captions {
                let mut caption = caption.clone();
                caption.x += origin.0;
                caption.y += origin.1;
                painter.draw(frame, &caption)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use shinki_emu_sdk::runtime::KeyModifiers;
    use std::{fs, io::Write};

    fn colored_crx(rgb: [u8; 3]) -> Vec<u8> {
        let mut bytes = Vec::from(*b"CRXG");
        for field in [0u16, 0, 1, 1, 2, 1, 0, 0] {
            bytes.extend(field.to_le_bytes());
        }
        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::fast());
        compressed.write_all(&[0, rgb[2], rgb[1], rgb[0]]).unwrap();
        bytes.extend(compressed.finish().unwrap());
        bytes
    }

    #[test]
    fn reading_an_original_format_mes_advances_the_composited_scene_after_input() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("Sample");
        let scripts = game.join("advdata/MES");
        let backgrounds = game.join("advdata/GRP/BG");
        let sprites = game.join("advdata/GRP/BST");
        fs::create_dir_all(&scripts).unwrap();
        fs::create_dir_all(&backgrounds).unwrap();
        fs::create_dir_all(&sprites).unwrap();
        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0xb6, 0xab, 0]);
        mes.extend(b"\x49bg\\first\0\x3c\x04bst\\actor\0\x3f\0.\0\x06\0\0\x49Bg\\second\0\0\0\0");
        fs::write(scripts.join("START.MES"), mes).unwrap();
        fs::write(backgrounds.join("FIRST.CRX"), colored_crx([25, 50, 75])).unwrap();
        fs::write(backgrounds.join("second.crx"), colored_crx([35, 55, 85])).unwrap();
        fs::write(sprites.join("ACTOR.CRX"), colored_crx([90, 110, 130])).unwrap();
        let mut vm = Vm::new(Game::find(&game).unwrap().unwrap(), None).unwrap();

        let first = vm.tick(Duration::ZERO, &[]).unwrap().unwrap();
        assert_eq!(&first[..4], &[90, 110, 130, 255]);
        assert!(vm.tick(Duration::from_secs(1), &[]).unwrap().is_none());
        let second = vm
            .tick(
                Duration::ZERO,
                &[RuntimeEvent::Key {
                    code: KeyCode::Enter,
                    state: KeyState::Pressed,
                    modifiers: KeyModifiers {
                        shift: false,
                        control: false,
                        alt: false,
                        super_key: false,
                    },
                }],
            )
            .unwrap()
            .unwrap();
        assert_eq!(&second[..4], &[35, 55, 85, 255]);
        assert!(vm.finished());
    }

    #[test]
    #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
    fn reset_window_size_preserves_the_preceding_display_mode_change() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("AdvData/MES");
        fs::create_dir_all(&scripts).unwrap();
        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0xb6, 0xaa, 0, 0x18, 8, 1, 0x83]);
        mes.extend([3u16, 0, 99, 4].into_iter().flat_map(u16::to_le_bytes));
        mes.extend([0x18, 8, 0, 0x83]);
        mes.extend([3u16, 1, 99, 4].into_iter().flat_map(u16::to_le_bytes));
        mes.extend([0x18, 27, 0, 0x10, 0, 0]);
        fs::write(scripts.join("START.MES"), mes).unwrap();
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.variable(3, 0).unwrap(), 1);
        assert_eq!(vm.variable(3, 1).unwrap(), 0);
        assert_eq!(
            vm.window_command(),
            Some(WindowCommand::SetFullscreen(true))
        );
        assert_eq!(
            vm.window_command(),
            Some(WindowCommand::SetFullscreen(false))
        );
        let (width, height) = vm.extent();
        assert_eq!(
            vm.window_command(),
            Some(WindowCommand::SetClientSize { width, height })
        );
        assert!(vm.window_command().is_none());
    }

    #[test]
    fn disc_check_uses_the_session_option() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("AdvData/MES");
        fs::create_dir_all(&scripts).unwrap();
        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0xb6, 0xaa, 0, 0x18, 10, 0, 0x10, 0, 0]);
        fs::write(scripts.join("START.MES"), mes).unwrap();

        let make_vm = || Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();
        assert!(make_vm().advance_timeline(Duration::ZERO, &[]).is_err());
        let mut vm = make_vm();
        vm.set_skip_disc_check(true);
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert!(vm.finished());
    }

    #[test]
    fn authored_button_drag_dispatches_only_on_a_drop_target() {
        for (version, button, text, drag, menu, label, copy, quit) in [
            (0xaab6u16, 0x72, 0x54, 0x33, 0x0b, 0x4b, 0x83, 0x10),
            (0xabb6, 0x72, 0x54, 0x33, 0x0b, 0x4b, 0x83, 0x10),
            (0x9f9a, 0x66, 0x4c, 0x32, 0x0a, 0x43, 0x76, 0x0f),
        ] {
            let root = tempfile::tempdir().unwrap();
            let scripts = root.path().join("AdvData/MES");
            fs::create_dir_all(&scripts).unwrap();
            let mut mes = Vec::from(1u32.to_le_bytes());
            mes.extend(3u32.to_le_bytes());
            mes.extend([0, 0]);
            mes.extend(version.to_le_bytes());
            mes.push(0);
            let sounds = if version == 0x9f9a { 0x15 } else { 0x17 };
            for (x, name, role) in [
                (0, b"assign".as_slice(), Some(0)),
                (20, b"".as_slice(), Some(1)),
                (40, b"".as_slice(), None),
                (60, b"".as_slice(), Some(1)),
            ] {
                mes.push(button);
                mes.extend([x, 0, x + 10, 10u16].into_iter().flat_map(u16::to_le_bytes));
                mes.push(text);
                mes.extend(name);
                mes.push(0);
                if role == Some(0) {
                    mes.extend([sounds, 10, 20, sounds, 255, 30]);
                }
                if let Some(role) = role {
                    mes.extend([drag, role, 0]);
                }
            }
            mes.extend([menu, 0, 0, label]);
            mes.extend(b"assign\0");
            mes.push(copy);
            mes.extend([3u16, 0, 99, 245].into_iter().flat_map(u16::to_le_bytes));
            mes.extend([quit, 0, 0, 6, 0, 0, 0, 0, 0]);
            fs::write(scripts.join("START.MES"), mes).unwrap();
            let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();

            let pointer = |x| RuntimeEvent::PointerMove { x, y: 5.0 };
            let primary = |state| RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state,
            };
            vm.advance_timeline(Duration::ZERO, &[]).unwrap();
            assert_eq!(vm.menu_buttons().unwrap()[0].sounds, [10, 30]);
            for (drop_x, finished) in [(45.0, false), (65.0, true)] {
                vm.advance_timeline(
                    Duration::ZERO,
                    &[
                        pointer(5.0),
                        primary(KeyState::Pressed),
                        pointer(drop_x),
                        primary(KeyState::Released),
                    ],
                )
                .unwrap();
                assert_eq!(vm.finished(), finished);
            }
            assert_eq!(vm.variable(3, 0).unwrap(), 1);
        }
    }

    #[test]
    fn alpha_hit_testing_uses_the_registered_atlas_region() {
        let mut atlas = RgbaImage::new(10, 10);
        atlas.put_pixel(5, 6, image::Rgba([0, 0, 0, 1]));
        let button = Button {
            area: [10, 20, 14, 24],
            hit_test_alpha: true,
            states: BTreeMap::from([(
                99,
                vec![Sprite {
                    source: Arc::new(Crx::from_image(0, 0, atlas)),
                    x: 10,
                    y: 20,
                    clip: Some(Clip {
                        x: 4,
                        y: 4,
                        width: 4,
                        height: 4,
                    }),
                    use_window_alpha: false,
                    effect: None,
                }],
            )]),
            ..Button::default()
        };
        assert!(button.hit_at(11, 22));
        assert!(!button.hit_at(12, 22));
        assert!(!button.hit_at(15, 22));
    }

    #[test]
    fn foreground_control_art_draws_after_later_registered_controls() {
        let make_button = |color, draw_on_top| Button {
            draw_on_top,
            states: BTreeMap::from([(
                99,
                vec![Sprite {
                    source: Arc::new(Crx::from_image(0, 0, RgbaImage::from_pixel(1, 1, color))),
                    x: 0,
                    y: 0,
                    clip: None,
                    use_window_alpha: false,
                    effect: None,
                }],
            )]),
            ..Button::default()
        };
        let foreground = image::Rgba([255, 60, 20, 255]);
        let buttons = [
            make_button(foreground, true),
            make_button(image::Rgba([20, 60, 255, 255]), false),
        ];
        let mut canvas = Canvas::new(1, 1, false);
        draw_buttons(
            &mut canvas,
            &buttons,
            None,
            false,
            None,
            (0, 0),
            &mut TextPainter::new(encoding_rs::SHIFT_JIS, None),
            None,
            |_| 0,
        )
        .unwrap();
        assert_eq!(*canvas.raster_mut().get_pixel(0, 0), foreground);
    }

    #[test]
    fn expanded_classic_scene_translation_animates_and_restores_the_stage() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("AdvData/MES");
        let backgrounds = root.path().join("AdvData/GRP/BG");
        fs::create_dir_all(&scripts).unwrap();
        fs::create_dir_all(&backgrounds).unwrap();
        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0x9a, 0x9f, 0]);
        mes.extend(b"\x42\\BG\\FIRST\0");
        mes.push(0x8a);
        mes.extend([1i16, 0, 0, 0].into_iter().flat_map(i16::to_le_bytes));
        mes.push(0x8d);
        mes.extend([1i16, 0, 0, -2].into_iter().flat_map(i16::to_le_bytes));
        mes.extend([6, 0, 0, 0x8d]);
        mes.extend([2i16, 0, 0, 0].into_iter().flat_map(i16::to_le_bytes));
        mes.extend([6, 0, 0, 0, 0, 0]);
        fs::write(scripts.join("START.MES"), mes).unwrap();
        fs::write(backgrounds.join("FIRST.CRX"), colored_crx([25, 50, 75])).unwrap();
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();

        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 0).0, [25, 50, 75, 255]);
        vm.advance_timeline(Duration::from_millis(100), &[])
            .unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 1).0, [25, 50, 75, 255]);
        vm.advance_timeline(Duration::from_millis(100), &[])
            .unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 2).0, [25, 50, 75, 255]);

        vm.advance(true).unwrap();
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 0).0, [25, 50, 75, 255]);
    }

    #[test]
    fn showing_a_registered_picture_and_leaving_a_scene_unlocks_both_galleries() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("AdvData");
        let scripts = data.join("MES");
        let graphics = data.join("GRP");
        let saves = data.join("SAVEDATA");
        fs::create_dir_all(&scripts).unwrap();
        fs::create_dir_all(graphics.join("BG")).unwrap();
        fs::create_dir_all(&saves).unwrap();

        let mut directory = vec![0; 12];
        for names in [
            [b"bg\\first".as_slice(), b"bg\\second".as_slice()].as_slice(),
            [b"start".as_slice()].as_slice(),
        ] {
            directory.extend((names.len() as u32).to_le_bytes());
            for name in names {
                directory.extend_from_slice(name);
                directory.resize(directory.len() + 32 - name.len(), 0);
            }
        }
        fs::write(graphics.join("NAMES.DAT"), directory).unwrap();
        fs::write(graphics.join("BG/FIRST.CRX"), colored_crx([25, 50, 75])).unwrap();
        fs::write(graphics.join("BG/SECOND.CRX"), colored_crx([35, 55, 85])).unwrap();

        for (file, code) in [
            (
                "START.MES",
                b"\x49BG\\FIRST\0\x49*1\0\x4fNEXT\0\0\0\0".as_slice(),
            ),
            ("NEXT.MES", b"\0\0\0".as_slice()),
        ] {
            let mut mes = Vec::from(1u32.to_le_bytes());
            mes.extend(3u32.to_le_bytes());
            mes.extend([0, 0, 0xb6, 0xab, 0]);
            mes.extend(code);
            fs::write(scripts.join(file), mes).unwrap();
        }
        let prefs = saves.join("system.dat");
        fs::write(&prefs, vec![0; 0x4b80 + 0x7f8]).unwrap();
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 0).0, [35, 55, 85, 255]);
        vm.flush_pending_saves().unwrap();
        let written = fs::read(prefs).unwrap();
        assert_eq!(&written[784..786], &[1, 1]);
        assert_eq!(written[1984], 1);
    }

    #[test]
    fn a_background_change_preserves_view_only_while_the_script_locks_it() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("AdvData/MES");
        let backgrounds = root.path().join("AdvData/GRP/BG");
        fs::create_dir_all(&scripts).unwrap();
        fs::create_dir_all(&backgrounds).unwrap();
        fs::write(backgrounds.join("FIRST.CRX"), colored_crx([25, 50, 75])).unwrap();
        fs::write(backgrounds.join("SECOND.CRX"), colored_crx([35, 55, 85])).unwrap();

        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0x9a, 0x9f, 0]);
        mes.extend(b"\x3a\x01BG\\FIRST\0\x42BG\\FIRST\0");
        mes.push(0x8d);
        mes.extend([0i16, 0, 0, -2].into_iter().flat_map(i16::to_le_bytes));
        mes.extend(b"\x42:n\0\x3a\x01BG\\SECOND\0\x42BG\\SECOND\0\x06\0\0");
        mes.extend(b"\x42:r\0\x3a\x01BG\\FIRST\0\x42BG\\FIRST\0\0\0\0");
        fs::write(scripts.join("START.MES"), mes).unwrap();
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();

        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 2).0, [35, 55, 85, 255]);
        vm.advance(true).unwrap();
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 0).0, [25, 50, 75, 255]);
    }

    #[test]
    fn preset_mask_fade_reveals_the_background() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("AdvData/MES");
        let backgrounds = root.path().join("AdvData/GRP/BG");
        let masks = root.path().join("AdvData/GRP/MSK");
        fs::create_dir_all(&scripts).unwrap();
        fs::create_dir_all(&backgrounds).unwrap();
        fs::create_dir_all(&masks).unwrap();
        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0x9a, 0x9f, 0]);
        mes.extend(b"\x42BG\\FIRST\0");
        mes.extend([0x0e, 17, 1, 6, 0, 0, 0, 0, 0]);
        fs::write(scripts.join("START.MES"), mes).unwrap();
        fs::write(backgrounds.join("FIRST.CRX"), colored_crx([25, 50, 75])).unwrap();
        fs::write(masks.join("MASK11A.CRX"), colored_crx([64; 3])).unwrap();
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();
        vm.set_variable(99, 10, 1).unwrap();

        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 0).0, [0, 0, 0, 255]);
        vm.advance_timeline(Duration::from_secs(1), &[]).unwrap();
        assert_eq!(vm.compose().unwrap().get_pixel(0, 0).0, [25, 50, 75, 255]);
    }

    #[test]
    fn dialogue_choices_branch_only_after_selection() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("AdvData/MES");
        fs::create_dir_all(&scripts).unwrap();
        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0xb6, 0xab, 0]);
        let boxes = [[100i16, 100, 500, 160], [100, 170, 500, 230]];
        for (area, label) in boxes.into_iter().zip([b":1", b":2"]) {
            mes.push(0x72);
            mes.extend(area.into_iter().flat_map(i16::to_le_bytes));
            mes.push(0x54);
            mes.extend(label);
            mes.push(0);
        }
        mes.extend([0x1a, 3, 0, 0x24, 3, 0]);
        mes.extend(b"\x09\x01\0\x5cfirst\0\x6c&IRST\0\x5csecond\0\x6c3ECOND\0\x0a\0\0");
        mes.extend(b"\x4bfirst\0\x82\x03\0\0\0\x01\0\0\0\x54end\0");
        mes.extend(b"\x4bsecond\0\x82\x03\0\0\0\x02\0\0\0\x4bend\0\0\0\0");
        fs::write(scripts.join("START.MES"), mes).unwrap();
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.variable(3, 0).unwrap(), 0);
        assert_eq!(vm.menu_buttons().unwrap().len(), 2);
        assert_eq!(vm.menu_buttons().unwrap()[0].area, boxes[0].map(i32::from));
        assert_eq!(vm.menu_buttons().unwrap()[1].area, boxes[1].map(i32::from));

        let inputs = [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::Enter].map(|code| {
            RuntimeEvent::Key {
                code,
                state: KeyState::Pressed,
                modifiers: KeyModifiers {
                    shift: false,
                    control: false,
                    alt: false,
                    super_key: false,
                },
            }
        });
        vm.advance_timeline(Duration::ZERO, &inputs).unwrap();
        assert_eq!(vm.variable(3, 0).unwrap(), 2);
        assert_eq!(vm.variable(99, 245).unwrap(), 1);
        assert!(vm.finished());

        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap(), None).unwrap();
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        vm.choices.as_mut().unwrap().push(Button {
            area: [400, 400, 500, 500],
            ..Button::default()
        });
        vm.pointer = (450, 450);

        vm.advance(false).unwrap();
        assert!(matches!(vm.wait, Wait::Menu));
        vm.advance_timeline(Duration::ZERO, &inputs[2..]).unwrap();
        assert_eq!(vm.variable(3, 0).unwrap(), 1);
        assert!(vm.finished());
    }
}
