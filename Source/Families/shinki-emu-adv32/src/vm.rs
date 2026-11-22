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
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TICK_BUDGET: usize = 8192;
const MAX_SURFACES: usize = 64;
const MESSAGE_MACROS: usize = 10;

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

#[derive(Clone)]
struct Button {
    area: [i32; 4],
    label: Vec<u8>,
    states: BTreeMap<u16, Sprite>,
    captions: BTreeMap<u16, Text>,
    save_slot: Option<u16>,
    list_index: Option<u16>,
}

impl Button {
    fn selectable(&self) -> bool {
        !self.label.is_empty() && self.label != b":n"
    }
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
    name_entry: Option<NameEntry>,
    sound_names: SoundNames,
    audio: Option<Audio>,
    null_audio: bool,
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
    button_group: u8,
    button_style: [u8; 2],
    button_highlight: bool,
    button_options: u16,
    button_routes: BTreeMap<u8, Vec<u8>>,
    menu_layers_enabled: bool,
    slot_rectangles: Vec<[i32; 4]>,
    scroll_list: Option<ScrollList>,
    slot_thumbnails: BTreeMap<u16, Arc<Crx>>,
    menu_position: (i32, i32, u16),
    confirmation_preference: Option<u16>,
    active_menu_origin: (i32, i32),
    menu_pan: Option<Pan>,
    keyboard_selection: Option<usize>,
    control_keys: u8,
    skip_dialogue: bool,
    auto_dialogue: bool,
    auto_elapsed: Duration,
    hidden_message: bool,
    backlog: VecDeque<(String, String)>,
    backlog_offset: Option<usize>,
    script_menu: Option<ScriptMenu>,
    last_voice: Option<String>,
    background: Option<Arc<Crx>>,
    background_position: (i32, i32),
    camera: Camera,
    camera_duration: u16,
    camera_motion: Option<CameraMotion>,
    scene_translation: (i32, i32),
    scene_translation_motion: Option<Pan>,
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
    transition_backdrop: Option<RgbaImage>,
    screen_break: Option<ScreenBreak>,
    cut_in: SceneLayer,
    overlays: Vec<Sprite>,
    overlay_text: Vec<Text>,
    gallery: Option<GalleryProfile>,
    gallery_portrait: Option<Arc<Crx>>,
    text_painter: Option<TextPainter>,
    blit_offset: (i32, i32),
    actors: BTreeMap<u16, Actor>,
    actor_wave_frames: u16,
    actor_wave_vertical: bool,
    actor_anchor: Option<(u16, i32, i32)>,
    actor_anchors: BTreeMap<(u16, String), (i32, i32)>,
    resume_offset: Option<usize>,
    pointer: (i32, i32),
    message: Option<Message>,
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
    display_preference: u8,
    window: Option<WindowCommand>,
    random: u32,
    frame_dirty: bool,
    wait: Wait,
}

impl Vm {
    pub(crate) fn new(game: Game) -> Result<Self> {
        let script = Arc::clone(&game.startup);
        let extent = match script.dialect.version {
            // Logical extent used by this engine dialect's display transform.
            0x9e97 | 0x9f9a | 0xa6b9 | 0xaab6 | 0xabb6 => (1280, 720),
            version => return Err(Error::UnsupportedMesVersion(version)),
        };
        let text_encoding = game.text_encoding();
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
            name_entry: None,
            sound_names,
            audio: None,
            null_audio: false,
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
            button_group: 0,
            button_style: [255, 255],
            button_highlight: true,
            button_options: 0,
            button_routes: BTreeMap::new(),
            menu_layers_enabled: false,
            slot_rectangles: Vec::new(),
            scroll_list: None,
            slot_thumbnails: BTreeMap::new(),
            menu_position: (0, 0, 0),
            confirmation_preference: None,
            active_menu_origin: (0, 0),
            menu_pan: None,
            keyboard_selection: None,
            control_keys: 0,
            skip_dialogue: false,
            auto_dialogue: false,
            auto_elapsed: Duration::ZERO,
            hidden_message: false,
            backlog: VecDeque::new(),
            backlog_offset: None,
            script_menu: None,
            last_voice: None,
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
            screen_break: None,
            cut_in: SceneLayer::default(),
            overlays: Vec::new(),
            overlay_text: Vec::new(),
            gallery: None,
            gallery_portrait: None,
            text_painter: None,
            blit_offset: (0, 0),
            actors: BTreeMap::new(),
            actor_wave_frames: 60,
            actor_wave_vertical: false,
            actor_anchor: None,
            actor_anchors: BTreeMap::new(),
            resume_offset: None,
            pointer: (-1, -1),
            message: None,
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
            display_preference: 0,
            window: None,
            random: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u32,
            frame_dirty: false,
            wait: Wait::Ready,
        })
    }

    pub(crate) fn set_null_audio(&mut self, enabled: bool) {
        self.null_audio = enabled;
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

    pub(crate) fn window_command(&mut self) -> Option<WindowCommand> {
        self.window.take()
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
        if self.gpu.is_some() {
            // Input, first frames and wait transitions are presented immediately.
            // A continuing animation can have one GPU frame in flight.
            let defer = elapsed != Duration::ZERO
                && events.is_empty()
                && previous_wait == std::mem::discriminant(&self.wait);
            return self.render_tick(defer);
        }
        if std::mem::take(&mut self.frame_dirty) {
            return Ok(Some(self.compose()?.into_raw().into()));
        }
        Ok(None)
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
            self.text_encoding == encoding_rs::GBK,
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
        let character_frames = self.variable(99, 26)?.clamp(0, 20) as u32;
        if let Some(message) = &mut self.message {
            self.frame_dirty |= message.advance(elapsed, character_frames);
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
            if effect.finished(elapsed) {
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
        for event in events {
            match event {
                RuntimeEvent::WindowFocused { focused: false } => self.control_keys = 0,
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
                    let before = self.hovered();
                    self.pointer = (*x as i32, *y as i32);
                    if self.hovered().is_some() {
                        self.keyboard_selection = None;
                    }
                    if before != self.hovered() && self.wait == Wait::Menu {
                        self.frame_dirty = true;
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
                    state: KeyState::Released,
                } => {
                    if !self.click_scrollbar()? && !self.invoke_input_route(2)? {
                        self.advance(false)?;
                    }
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Secondary,
                    state: KeyState::Released,
                } => {
                    if !self.invoke_input_route(3)? {
                        self.toggle_story_menu()?;
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
            self.auto_elapsed = self.auto_elapsed.saturating_add(elapsed);
            let speed = self.variable(99, 29)?.clamp(0, 22) as u64;
            let count = message.parts().0.chars().count() as u64;
            let delay = Duration::from_millis(5 * (23 - speed) * count.max(1));
            let voice = self.audio.as_ref().is_some_and(|audio| audio.playing(1));
            if self.auto_elapsed >= delay && !voice {
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
            && self.camera_motion.is_none()
            && self.scene_translation_motion.is_none()
            && !self
                .panning_planes
                .values()
                .any(|plane| plane.motions.iter().any(Option::is_some))
            && self.transition.is_none()
            && self.screen_break.is_none()
            && !self.cut_in.active()
            && !self
                .actors
                .values()
                .any(|actor| actor.layer.active() || actor.motion.is_some() || actor.wave.is_some())
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
                let target = self.menu_buttons().and_then(|buttons| {
                    let selected = if keyboard {
                        self.keyboard_selection
                            .or_else(|| self.hovered())
                            .or_else(|| {
                                buttons
                                    .iter()
                                    .position(|button| self.menu_selectable(button))
                            })
                    } else {
                        self.hovered()
                    };
                    buttons.get(selected?).map(|button| {
                        (
                            button.label.clone(),
                            button.save_slot,
                            button.list_index,
                            button.area,
                        )
                    })
                });
                if let Some((label, slot, list_index, area)) = target
                    && !label.is_empty()
                {
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
                    if label[0] == b':' {
                        return self.menu_action(&label);
                    }
                    self.jump(&label)?;
                    self.choices = None;
                    self.confirmation_preference = None;
                    self.wait = Wait::Ready;
                    self.frame_dirty = true;
                    self.active_menu_origin = (0, 0);
                    self.menu_pan = None;
                    self.keyboard_selection = None;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn menu_action(&mut self, action: &[u8]) -> Result<()> {
        if let Some(preference) = self.confirmation_preference {
            match action {
                b":y" | b":n" => {
                    let selected = u8::from(action == b":n");
                    if let Some(label) = self.button_routes.get(&selected).cloned() {
                        self.jump(&label)?;
                    }
                    self.confirmation_preference = None;
                    self.wait = Wait::Ready;
                    self.frame_dirty = true;
                    return Ok(());
                }
                b":c" => {
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
            b":qs" | b":aaqsave" => self.save_slot(2)?,
            b":ql" | b":aaqload" => self.load_slot(2)?,
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
            b":voice" => {
                if let Some(voice) = self.last_voice.clone() {
                    self.play_sound(&voice, 1, false)?;
                }
            }
            b":aawin_size" => {
                self.display_preference ^= 1;
                self.window = Some(WindowCommand::SetFullscreen(self.display_preference != 0));
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
        button.selectable() || (self.confirmation_preference.is_some() && button.label == b":n")
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
                label: Vec::new(),
                states: BTreeMap::new(),
                captions: BTreeMap::new(),
                save_slot: None,
                list_index: None,
            }
        });
        button.label = label.to_vec();
        if button.captions.is_empty() {
            let [left, top, right, bottom] = button.area;
            button.captions.insert(
                99,
                Text {
                    content: caption,
                    x: left + 24,
                    y: (top + bottom) / 2 - 14,
                    width: Some((right - left - 48).max(1) as u32),
                    size: 26,
                    color: [255; 3],
                },
            );
        } else {
            for text in button.captions.values_mut() {
                text.content.clone_from(&caption);
            }
        }
        choices.push(button);
    }

    fn toggle_story_menu(&mut self) -> Result<()> {
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
        self.switch_script(&name)?;
        self.backlog_offset = (system == 6).then_some(0);
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
        self.confirmation_preference = None;
        self.keyboard_selection = None;
        self.active_menu_origin = (0, 0);
        self.wait = menu.return_wait;
        Ok(true)
    }

    fn scroll_backlog(&mut self, towards_newer: bool) {
        if let Some(offset) = &mut self.backlog_offset {
            *offset = if towards_newer {
                offset.saturating_sub(1)
            } else {
                offset
                    .saturating_add(1)
                    .min(self.backlog.len().saturating_sub(1))
            };
            self.frame_dirty = true;
        }
    }

    fn log_message(&mut self, line: String, speaker: String) {
        while self.backlog.len() >= 2048 {
            self.backlog.pop_front();
        }
        self.backlog.push_back((speaker, line));
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

    fn hovered(&self) -> Option<usize> {
        let (x, y) = self.pointer;
        let x = x - self.active_menu_origin.0;
        let y = y - self.active_menu_origin.1;
        self.menu_buttons()?.iter().rposition(|button| {
            let [left, top, right, bottom] = button.area;
            (left..right).contains(&x) && (top..bottom).contains(&y)
        })
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
            for state in button.states.values_mut() {
                state.x += area[0];
                state.y += area[1];
            }
            for caption in button.captions.values_mut() {
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
                    Text {
                        content: title.clone(),
                        x: area[0] + 16,
                        y: area[1] + 8,
                        size: 22,
                        color: [255; 3],
                        width: Some((area[2] - area[0]).saturating_sub(32).max(1) as u32),
                    },
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
            let painter = self.text_painter.get_or_insert_with(TextPainter::new);
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
                            content: text.to_owned(),
                            x: x + offset,
                            y: top,
                            size: 30,
                            color,
                            width: Some(400),
                        },
                    );
                }
            }
            painter.draw(
                &mut frame,
                &Text {
                    content: confirm.to_owned(),
                    x,
                    y: y + 138,
                    size: 20,
                    color: [255; 3],
                    width: Some(500),
                },
            );
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
        layers.push((&self.cut_in, (0, 0), None));
        layers.sort_by_key(|(layer, ..)| layer.order);
        for (layer, offset, accessory) in layers {
            layer.draw(&mut frame, offset);
            if let Some(texture) = accessory {
                layer.draw_texture(&mut frame, texture, offset);
            }
        }
        if let Some(portrait) = &self.gallery_portrait {
            graphics::blit_alpha(&mut frame, portrait, 408, -137, None, 255, 50);
        }
        for sprite in &self.overlays {
            match sprite.effect {
                None => graphics::blit(&mut frame, &sprite.source, sprite.x, sprite.y, sprite.clip),
                Some(SpriteEffect::Entrance { dx, dy, remaining }) => {
                    let fraction = remaining.div_duration_f64(SPRITE_ENTRANCE);
                    let dx = (f64::from(dx) * fraction).round() as i32;
                    let dy = (f64::from(dy) * fraction).round() as i32;
                    let opacity = ((1.0 - fraction) * 255.0).round() as u8;
                    graphics::blit_alpha(
                        &mut frame,
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
                    graphics::blit(
                        &mut frame,
                        &sprite.source,
                        sprite.x + dx,
                        sprite.y + dy,
                        sprite.clip,
                    );
                }
            }
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
        if self.wait == Wait::Menu
            && let Some(buttons) = self
                .choices
                .as_deref()
                .or_else(|| self.parties.get(&self.button_group).map(Vec::as_slice))
        {
            let hovered = self.keyboard_selection.or_else(|| self.hovered());
            for (index, button) in buttons.iter().enumerate() {
                let state = if (self.button_highlight || self.choices.is_some())
                    && hovered == Some(index)
                {
                    2
                } else {
                    1
                };
                if let Some(sprite) = button.states.get(&state).or_else(|| button.states.get(&99)) {
                    graphics::blit(
                        &mut frame,
                        &sprite.source,
                        sprite.x + self.active_menu_origin.0,
                        sprite.y + self.active_menu_origin.1,
                        sprite.clip,
                    );
                }
                if let Some(caption) = button
                    .captions
                    .get(&state)
                    .or_else(|| button.captions.get(&99))
                {
                    let mut caption = caption.clone();
                    caption.x += self.active_menu_origin.0;
                    caption.y += self.active_menu_origin.1;
                    self.text_painter
                        .get_or_insert_with(TextPainter::new)
                        .draw(&mut frame, &caption);
                }
            }
        }
        if !self.overlay_text.is_empty() {
            let painter = self.text_painter.get_or_insert_with(TextPainter::new);
            for text in &self.overlay_text {
                painter.draw(&mut frame, text);
            }
        }
        if !self.hidden_message
            && let Some(message) = &self.message
        {
            let top = if let Some(panel) = self.message_panel
                && let Some(texture) = self.surfaces.get(&self.texture_slots.panel)
            {
                let top = height.saturating_sub(panel.height) as i32;
                graphics::blit(&mut frame, texture, 0, top, Some(panel));
                top
            } else {
                (height * 3 / 4) as i32
            };
            let painter = self.text_painter.get_or_insert_with(TextPainter::new);
            for text in message.text(top, width, &self.palette) {
                painter.draw(&mut frame, &text);
            }
        }
        if let Some(comments) = &self.comments {
            comments.draw(
                &mut frame,
                self.text_painter.get_or_insert_with(TextPainter::new),
            );
        }
        if let Some(offset) = self.backlog_offset {
            let painter = self.text_painter.get_or_insert_with(TextPainter::new);
            let speaker_color = self.palette.get(&15).copied().unwrap_or([255; 3]);
            let mut y = 78;
            for (speaker, line) in self.backlog.iter().rev().skip(offset).take(6).rev() {
                if !speaker.is_empty() {
                    painter.draw(
                        &mut frame,
                        &Text {
                            content: speaker.clone(),
                            x: 100,
                            y,
                            width: Some(1080),
                            size: 20,
                            color: speaker_color,
                        },
                    );
                    y += 24;
                }
                painter.draw(
                    &mut frame,
                    &Text {
                        content: line.clone(),
                        x: 100,
                        y,
                        width: Some(1080),
                        size: 22,
                        color: [255; 3],
                    },
                );
                y += 62;
            }
        }
        if let Some(fade) = &self.fade {
            let fraction = fade.remaining.div_duration_f64(fade.total);
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
                    self.saves.mark_read(scene, self.tick_label)?;
                }
            }
            0x05 => self.control_music_volume(a)?,
            0x06 => {
                if !self
                    .comments
                    .as_ref()
                    .is_some_and(|comments| comments.visible)
                {
                    self.wait = Wait::Input;
                    self.frame_dirty = self.background.is_some();
                }
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
                    self.skip_dialogue = false;
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
                self.open_menu(self.button_group);
                self.refresh_scroll_list(self.variable(99, 246)?)?;
            }
            0x0f if matches!(a, 7 | 8 | 10 | 11 | 13 | 14) => {}
            0x0f if matches!(a, 0 | 1) => self.fade_in(b)?,
            0x0f if a == 3 => self.fade_color = [255; 3],
            0x0f if matches!(a, 4 | 6) => self.shake(None, a == 6, true),
            0x0f if a == 5 => self.frame_dirty |= self.background.is_some(),
            0x0f if a == 9 => self.transition_backdrop = Some(self.compose()?),
            0x0f if matches!(a, 15 | 16) => {
                if self.control_keys == 0
                    && !self.skip_dialogue
                    && let Some(frame) = self.transition_backdrop.take()
                {
                    self.screen_break = Some(ScreenBreak::new(frame, a == 15, &mut self.random));
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
            0x17 => self.button_style = [a, b],
            0x13 => self.stop_music()?,
            0x18 if a == 0 => {
                let slot = self.variable(99, 254)? as u16;
                self.load_slot(slot)?;
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
            0x18 if a == 8 => self.window = Some(WindowCommand::SetFullscreen(b != 0)),
            // This dialect calls the startup checker and discards its result.
            0x18 if a == 10 && self.script.dialect.version == 0x9f9a => {}
            0x18 if a == 15 => self.input_mode = b,
            0x18 if a == 18 => self.menu_layers_enabled = true,
            0x18 if a == 20 && self.script.dialect.version == 0x9f9a => {
                self.saves.reset_classic_settings()?;
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
            0x18 if a == 27 => self.frame_dirty = true,
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
                self.parties.insert(a, buttons);
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
                    self.variables
                        .retain(|&(bank, index), _| !self.saves.is_global(bank, index));
                }
            }
            0x1e if a == 19 => {
                let slot = self.variable(99, 254)? as u16;
                self.set_variable(9, 87, i32::from(!self.saves.exists(slot)))?;
            }
            0x1e if a == 20 => {
                self.slot_thumbnails.clear();
                if b == 0 {
                    let first = 3 + 10 * self.variable(99, 0)?.clamp(0, 9) as u16;
                    for slot in first..first + 10 {
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
            0x1e if a == 51 && self.script.dialect.classic() => {
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
            // Unassigned button-bank selectors leave the current bank alone.
            0x24 if matches!(a, 0 | 8..=12 | 17..=253) => {}
            0x24 => {
                let buttons = self.parties.remove(&0).unwrap_or_default();
                self.parties.insert(a, buttons);
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
            (0x49, None) if bytes == b":n" || bytes == b":r" => {}
            (0x49, None) => {
                self.background = Some(if bytes == b"--" {
                    self.surface(1)?
                } else {
                    self.graphics.load(&self.game, &decode_japanese(bytes)?)?
                });
                self.background_position = (0, 0);
                self.centered_background = std::mem::take(&mut self.center_next_background);
                self.fitted_background = None;
                self.pan = None;
                self.overlays.clear();
                self.overlay_text.clear();
                self.frame_dirty = true;
            }
            (0x4b, None) => {}
            (0x4c | 0x4d | 0x4e, None) => {
                let name = decode_japanese(bytes)?;
                if name == "-" {
                    self.stop_sound(1)?;
                } else if self.control_keys == 0 && !self.skip_dialogue {
                    self.play_sound(&name, 1, false)?;
                    self.last_voice = Some(name);
                }
            }
            (0x4f | 0x50, None) => self.switch_script(bytes)?,
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
                    self.confirmation_preference = Some(self.menu_position.2);
                    self.open_menu(2);
                }
            }
            (0x5a, None) => self.open_menu(16),
            (0x5b, None) => self.open_menu(17),
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
                    self.log_message(caption, decode_japanese(bytes)?);
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
            (0x6a, None) => self.speaker = Script::decode_text(bytes, self.text_encoding)?,
            (0x6b, None) => {
                let message = Message::new(
                    bytes,
                    std::mem::take(&mut self.speaker),
                    &self.player_names,
                    self.text_encoding,
                )?;
                let (line, speaker) = message.parts();
                self.log_message(line.to_owned(), speaker.to_owned());
                self.message = Some(message);
                self.auto_elapsed = Duration::ZERO;
                self.frame_dirty = true;
                if self
                    .script
                    .instruction(self.ip)
                    .ok()
                    .and_then(|next| self.script.dialect.opcode(next.opcode))
                    != Some(0x06)
                {
                    self.wait = Wait::Input;
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
                        states: BTreeMap::new(),
                        captions: BTreeMap::new(),
                        save_slot: None,
                        list_index: None,
                    });
            }
            0x76 => {
                let operand = self.script.instruction(self.ip)?;
                let Operands::Text { bytes, .. } = operand.args else {
                    return Err(Error::Invalid("ADV32 menu caption operand"));
                };
                let content = Script::decode_text(bytes, self.text_encoding)?;
                self.ip = operand.next;
                if !content.is_empty() {
                    let palette = (a[2] & 0x7f) + 12;
                    let color = self.palette.get(&palette).copied().unwrap_or([255; 3]);
                    let caption = Text {
                        content,
                        x: signed[0],
                        y: signed[1],
                        size: 24,
                        color,
                        width: None,
                    };
                    let button = self
                        .parties
                        .get_mut(&self.button_group)
                        .and_then(|buttons| buttons.last_mut())
                        .ok_or(Error::Invalid("ADV32 caption outside a button"))?;
                    button.captions.insert(a[3], caption);
                }
            }
            0x75 if !self.menu_layers_enabled
                && self.script.dialect.version != 0x9f9a
                && self
                    .scroll_list
                    .as_ref()
                    .is_none_or(|list| list.items.is_empty()) => {}
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
                    effect: None,
                };
                if code == 0x73 {
                    let button = self
                        .parties
                        .get_mut(&self.button_group)
                        .and_then(|buttons| buttons.last_mut())
                        .ok_or(Error::Invalid("ADV32 menu image outside a button"))?;
                    button.states.insert(tail[3], sprite);
                } else if code == 0x75
                    && tail[3] != u16::MAX
                    && let Some(item) = self
                        .scroll_list
                        .as_mut()
                        .and_then(|list| list.items.last_mut())
                {
                    item.states.insert(tail[3], sprite);
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
                        area: [0; 4],
                        label: Vec::new(),
                        states: BTreeMap::new(),
                        captions: BTreeMap::new(),
                        save_slot: None,
                        list_index: Some(a[0]),
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
                        Text {
                            content: caption.into_owned(),
                            x: signed[0],
                            y: signed[1],
                            size: 22,
                            color: [255; 3],
                            width: None,
                        },
                    );
                }
            }
            0x81 => self.button_style = [a[0] as u8, a[1] as u8],
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
                let (frames, offset) = match a[0] {
                    0 => (12, 0),
                    1 => (12, -20),
                    2 => (12, 20),
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
                let total = Duration::from_micros(u64::from(a[2]) * 1_000_000 / 60);
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

    fn control_comments(&mut self, [command, value, ..]: [u16; 4]) -> Result<()> {
        if command == 99 {
            let background = if value == 1 {
                match self.graphics.load(&self.game, "SYSTEM\\COMMENTBACK") {
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
                Some(Comments::new(value, background)?)
            };
        } else if let Some(comments) = &mut self.comments {
            match command {
                0 => comments.reset(false),
                u16::MAX => comments.reset(true),
                1 => {
                    comments.reset(true);
                    comments.visible = true;
                }
                2 => comments.set_period(value),
                3 => comments.set_random_order(value != 0),
                6 => comments.set_repeat(value == 0),
                7 => comments.visible = true,
                98 => comments.visible = false,
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
            let mut name = if let Some(number) = bytes.strip_prefix(b"*") {
                let number = std::str::from_utf8(number)
                    .ok()
                    .and_then(|value| value.parse::<u16>().ok())
                    .ok_or(Error::Invalid("ADV32 picture number"))?;
                self.sound_names.picture(number)?.to_owned()
            } else {
                decode_japanese(bytes)?
            };
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
                .and_then(|image| message::panel(image));
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
            resume: self.resume_offset.unwrap_or(self.ip),
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
            fog_enabled: self.fog_enabled,
            player_names: self.player_names.clone(),
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
        self.tick_label = snapshot.entry;
        self.tick_step = snapshot.entry_step;
        self.next_label = None;
        self.variables.retain(|&(bank, _), _| bank > 10);
        self.variables.extend(snapshot.variables);
        self.surface_names.clear();
        self.previous_accessories.clear();
        self.surfaces.clear();
        self.actors.clear();
        self.texture_variant = snapshot.texture_variant;
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
            comments.restore(snapshot.comments, self.text_encoding)?;
        }
        self.animation_sources.clear();
        self.animation = None;
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
            .then(|| Message::resumed(snapshot.line, snapshot.speaker, &self.player_names))
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

    fn open_menu(&mut self, group: u8) {
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
            self.wait = Wait::Menu;
            self.active_menu_origin = (self.menu_position.0, self.menu_position.1);
            self.keyboard_selection = None;
            self.frame_dirty = true;
        }
    }

    fn queue_save_grid(&mut self, loading: bool) -> Result<()> {
        if self.slot_rectangles.is_empty() || self.script.label(b"selsave", 0x4b)?.is_none() {
            return Ok(());
        }
        let first = 3 + 10 * self.variable(99, 0)?.clamp(0, 9) as u16;
        let buttons = self.parties.entry(self.button_group).or_default();
        buttons.retain(|button| button.save_slot.is_none());
        for (index, &area) in self.slot_rectangles.iter().take(10).enumerate() {
            let slot = first + index as u16;
            if loading && !self.saves.exists(slot) {
                continue;
            }
            let mut states = BTreeMap::new();
            if let Some(source) = self.slot_thumbnails.get(&slot) {
                let thumbnail_height = source.image.height() as i32;
                states.insert(
                    99,
                    Sprite {
                        source: source.clone(),
                        x: area[0] + 12,
                        y: area[1] + (area[3] - area[1] - thumbnail_height) / 2,
                        clip: None,
                        effect: None,
                    },
                );
            }
            buttons.push(Button {
                area,
                label: b"selsave".to_vec(),
                states,
                captions: BTreeMap::new(),
                save_slot: Some(slot),
                list_index: None,
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

    fn queue_overlay_text(&mut self, [x, y, size, palette]: [u16; 4]) -> Result<()> {
        let operand = self.script.instruction(self.ip)?;
        let Operands::Text { bytes, .. } = operand.args else {
            return Err(Error::Invalid("ADV32 overlay text operand"));
        };
        let content = Script::decode_text(bytes, self.text_encoding)?;
        self.ip = operand.next;
        if content.is_empty() {
            return Ok(());
        }
        let palette = palette & 0x7f;
        let color = self
            .palette
            .get(&(palette + 12))
            .copied()
            .unwrap_or([255; 3]);
        self.overlay_text.push(Text {
            content,
            x: i32::from(x as i16),
            y: i32::from(y as i16),
            size: size.min(98),
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
        if self.audio.is_none() {
            self.audio = Some(Audio::open(self.null_audio)?);
        }
        self.audio
            .as_mut()
            .expect("audio was opened")
            .load_and_play(&self.game, name, channel, repeat, gain, pan)
    }

    fn play_effect(&mut self, index: u16, repeat: bool) -> Result<()> {
        let name = self.sound_names.effect(index)?.to_owned();
        let channel = self.next_effect_channel;
        self.play_sound(&name, channel, repeat)?;
        self.next_effect_channel = if channel == 5 { 2 } else { channel + 1 };
        Ok(())
    }

    #[cfg(feature = "video-ffmpeg")]
    fn start_movie(&mut self, name: &str) -> Result<()> {
        let path = self.game.resource("movie", name, "mpg")?.to_path_buf();
        if self.audio.is_none() {
            self.audio = Some(Audio::open(self.null_audio)?);
        }
        self.stop_music()?;
        for channel in 1..=5 {
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
        for channel in 0..=5 {
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
        self.classic_choice_text.clear();
        self.gallery_portrait = None;
        self.ip = entry;
        self.music_end_route = None;
        self.scroll_list = None;
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
        self.variables.insert((bank, index), value);
        self.saves.update_global(bank, index, value);
        if bank == 99 && self.audio.is_some() {
            let channels: &[u8] = match index {
                7 | 14 => &[0],
                5 | 9 => &[1],
                8 | 15 => &[2, 3, 4, 5],
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
                self.variables.insert((99, 11), value);
                self.saves.update_global(99, 11, value);
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
        fs::create_dir_all(&scripts).unwrap();
        fs::create_dir_all(&backgrounds).unwrap();
        let mut mes = Vec::from(1u32.to_le_bytes());
        mes.extend(3u32.to_le_bytes());
        mes.extend([0, 0, 0xb6, 0xab, 0]);
        mes.extend(b"\x49bg\\first\x00\x06\x00\x00\x49Bg\\second\x00\x00\x00\x00");
        fs::write(scripts.join("START.MES"), mes).unwrap();
        fs::write(backgrounds.join("FIRST.CRX"), colored_crx([25, 50, 75])).unwrap();
        fs::write(backgrounds.join("second.crx"), colored_crx([35, 55, 85])).unwrap();
        let mut vm = Vm::new(Game::find(&game).unwrap().unwrap()).unwrap();

        let first = vm.tick(Duration::ZERO, &[]).unwrap().unwrap();
        assert_eq!(&first[..4], &[25, 50, 75, 255]);
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
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap()).unwrap();

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
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap()).unwrap();
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
        mes.extend(b"\x09\x01\0\x5cfirst\0\x6c&IRST\0\x5csecond\0\x6c3ECOND\0\x0a\0\0");
        mes.extend(b"\x4bfirst\0\x82\x03\0\0\0\x01\0\0\0\x54end\0");
        mes.extend(b"\x4bsecond\0\x82\x03\0\0\0\x02\0\0\0\x4bend\0\0\0\0");
        fs::write(scripts.join("START.MES"), mes).unwrap();
        let mut vm = Vm::new(Game::find(root.path()).unwrap().unwrap()).unwrap();
        vm.advance_timeline(Duration::ZERO, &[]).unwrap();
        assert_eq!(vm.variable(3, 0).unwrap(), 0);
        assert_eq!(vm.menu_buttons().unwrap().len(), 2);

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
        assert!(vm.finished());
    }
}
