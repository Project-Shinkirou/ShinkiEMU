use crate::{Game, Graphics, NativeCall, Step, Word};
use roukaku_text::{LayoutLine, LogicalTextLayout};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::text::{Font, Text};

pub struct Host {
    game: Game,
    graphics: Graphics,
    menu: crate::menu::Menu,
    animations: crate::animation::Animations,
    strips: crate::strip::Strips,
    effects: crate::effects::Effects,
    root: std::path::PathBuf,
    app_build_id: Option<Vec<u8>>,
    archive_key: Option<(u32, u32)>,
    global_save: Option<String>,
    text_log_file: Option<String>,
    text_log: Option<crate::state::TextLog>,
    system: crate::app::System,
    timers: crate::timing::Timers,
    options: [i32; 12],
    active_host_option: bool,
    low_priority_requests: i32,
    display_mode: i32,
    application_options: crate::app::AppConfig,
    settings_changed: bool,
    backlog_limit: usize,
    backlog_enabled: bool,
    dialogue_enabled: bool,
    dialogue: crate::dialogue::Dialogue,
    fonts: BTreeMap<i32, Font>,
    font: Option<i32>,
    input: crate::input::Input,
    text_encoding: &'static encoding_rs::Encoding,
    text: Option<Text>,
    slot_signature: Option<[u8; 4]>,
    slot_history: crate::state::SlotHistory,
    slot_chapter: [u8; 44],
    slot_summary: [u8; 144],
    slot_labels: [Option<crate::Address>; 2],
    slot_checkpoint: Option<Vec<u8>>,
    thumbnail_conditions: [Option<crate::Address>; 2],
    thumbnail_surface: Option<i32>,
    audio: Option<shinki_emu_sdk::NativeAudioOutput>,
    audio_stop: Arc<AtomicBool>,
    null_audio: bool,
    channels: crate::sound::Channels,
    voices: Option<crate::voice::Voices>,
    random: crate::random::Random,
}

impl Host {
    pub fn open(root: &Path) -> io::Result<Self> {
        let mut game = Game::open(root)?;
        let data = game.assets_mut().read("script/ll_app.sys", 4096)?;
        let system = crate::app::System::read(root, &data)?;
        let application_options = crate::app::AppConfig::open(&system.save_root)?;
        let (options, display_mode) = crate::app::read_options(root, &application_options)?;
        let voices = match game
            .assets_mut()
            .read("script/voiceinf.dat", 16 * 1024 * 1024)
        {
            Ok(bytes) => Some(crate::voice::Voices::parse(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            game,
            graphics: Graphics::open(system.width, system.height)?,
            menu: crate::menu::Menu::default(),
            animations: crate::animation::Animations::default(),
            strips: crate::strip::Strips::default(),
            effects: crate::effects::Effects::default(),
            root: root.to_owned(),
            app_build_id: None,
            archive_key: None,
            global_save: None,
            text_log_file: None,
            text_log: None,
            system,
            timers: crate::timing::Timers::default(),
            options,
            active_host_option: options[1] != 0,
            low_priority_requests: 0,
            display_mode,
            application_options,
            settings_changed: true,
            backlog_limit: 0,
            backlog_enabled: false,
            dialogue_enabled: true,
            dialogue: crate::dialogue::Dialogue::default(),
            fonts: BTreeMap::new(),
            font: None,
            input: crate::input::Input::default(),
            text_encoding: encoding_rs::SHIFT_JIS,
            text: None,
            slot_signature: None,
            slot_history: crate::state::SlotHistory::default(),
            slot_chapter: [0; 44],
            slot_summary: [0; 144],
            slot_labels: [None, None],
            slot_checkpoint: None,
            thumbnail_conditions: [None, None],
            thumbnail_surface: None,
            audio: None,
            audio_stop: Arc::new(AtomicBool::new(false)),
            null_audio: false,
            channels: crate::sound::Channels::default(),
            voices,
            random: crate::random::Random::seeded(
                (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    % 1_000_000_000) as i32,
            ),
        })
    }

    pub fn advance(&mut self, budget: usize) -> io::Result<Step> {
        let step = self.game.advance(budget)?;
        if matches!(step, Step::Yield | Step::Finished) {
            self.graphics
                .update_transitions(self.timers.now(), &mut self.random);
            self.animations
                .tick(self.timers.now(), &mut self.graphics)?;
            self.strips
                .tick(self.timers.now(), &mut self.graphics, &mut self.random);
            self.effects
                .tick(self.timers.now(), &mut self.graphics, &mut self.random);
            self.graphics.present_display()?;
            self.input.next_tick();
            if let Some(audio) = &mut self.audio {
                self.channels
                    .mix(self.timers.now(), audio, &self.audio_stop)?;
            }
        }
        let Step::Calling(call) = step else {
            return Ok(step);
        };
        let result = self.call(call).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Lucifen native call {}: {error}", call.function),
            )
        })?;
        let Some(result) = result else {
            return Ok(step);
        };
        self.game.vm_mut().complete_call(result)?;
        Ok(Step::BudgetExhausted)
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub(crate) fn configure_audio(&mut self, stop: Arc<AtomicBool>, null_audio: bool) {
        self.audio_stop = stop;
        self.null_audio = null_audio;
    }

    pub(crate) fn receive_input(&mut self, input: Vec<shinki_emu_sdk::runtime::RuntimeEvent>) {
        for event in input {
            self.input.event(event);
        }
    }

    pub(crate) fn set_suspended(&mut self, paused: bool) {
        self.timers.set_suspended(paused);
    }

    pub fn display_size(&self) -> (u32, u32) {
        (self.system.width, self.system.height)
    }

    pub fn set_text_encoding(&mut self, encoding: &str) -> io::Result<()> {
        self.text_encoding = match encoding {
            "cp932" => encoding_rs::SHIFT_JIS,
            "gbk" => encoding_rs::GBK,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid Lucifen text_encoding",
                ));
            }
        };
        self.text = None;
        Ok(())
    }

    pub fn decode_text<'a>(&self, text: &'a [u8]) -> std::borrow::Cow<'a, str> {
        self.text_encoding.decode_without_bom_handling(text).0
    }

    pub fn current_font(&self) -> Option<(&str, u16)> {
        let font = self.fonts.get(&self.font?)?;
        Some((&font.family, font.size))
    }

    pub fn graphics(&self) -> &Graphics {
        &self.graphics
    }

    fn call(&mut self, call: NativeCall) -> io::Result<Option<Word>> {
        let result = match call.function {
            // Lucifen's environment query reports whether the alpha drawing
            // backend was initialized (selector 21).
            6 if self.arg(call, 0)?.integer()? == 9 => self.display_mode,
            6 if self.arg(call, 0)?.integer()? == 21 => {
                i32::from(self.graphics.alpha_compositing())
            }
            4 => {
                let option = self.arg(call, 0)?.integer()? as usize;
                if option < self.options.len() {
                    let value = self.arg(call, 1)?.integer()?;
                    self.options[option] = match option {
                        1 => i32::from(value != 0),
                        3 | 6 => i32::from(value as u16),
                        _ => i32::from(value as u8),
                    };
                    if option == 1 {
                        self.active_host_option = value != 0;
                    }
                }
                0
            }
            5 if self.arg(call, 0)?.integer()? == 12 => {
                if self.root.join("SETUP.IFG").try_exists()? {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "Lucifen installer metadata requires native registration",
                    ));
                }
                0
            }
            5 => {
                let option = self.arg(call, 0)?.integer()? as usize;
                let Some(value) = self.options.get(option) else {
                    return Ok(None);
                };
                *value
            }
            7 => self.environment_path(call)?,
            8 | 9 => {
                let group = self.arg(call, 0)?.integer()?;
                let option = self.arg(call, 1)?.integer()? as u8;
                if group == 2 && option == 0 {
                    if call.function == 8 {
                        let address = self.arg(call, 2)?.address()?;
                        let font = self.game.vm().text(address)?;
                        self.application_options.set_font_name(&font);
                        self.settings_changed = true;
                        return Ok(Some(Word::Integer(0)));
                    }
                    let name = self.application_options.font_name();
                    return Ok(Some(self.game.vm_mut().host_text(name)));
                }
                if call.function == 8 {
                    let value = self.arg(call, 2)?.integer()?;
                    if self.application_options.set(group, option, value) {
                        self.settings_changed = true;
                        0
                    } else {
                        1
                    }
                } else {
                    self.application_options.get(group, option).unwrap_or(0)
                }
            }
            10 => {
                let index = self.arg(call, 0)?.integer()? as usize;
                let setting = self.arg(call, 1)?.integer()?;
                let value = self.arg(call, 2)?.integer()?;
                self.voices
                    .as_mut()
                    .map_or(-1, |voices| voices.set_group(index, setting, value))
            }
            12 => {
                let changed = self.settings_changed;
                if !changed && self.arg(call, 0)?.integer()? != 0 {
                    return Ok(Some(Word::Integer(0)));
                }
                let consume = self.arg(call, 1)?.integer()? != 0;
                let scroll = self.arg(call, 2)?.integer()?;
                let list = self.arg(call, 3)?.integer()?;
                let mode = self.application_options.get(2, 6).unwrap_or(0) != 0;
                let [red, green, blue] =
                    packed_rgb(self.application_options.get(2, 8).unwrap_or(0) as u32);
                let alpha = self.application_options.get(2, 7).unwrap_or(255) as u8;
                self.menu.set_message_appearance(
                    scroll,
                    list,
                    mode,
                    [red, green, blue, alpha],
                    true,
                    &mut self.graphics,
                )?;
                if self.arg(call, 5)?.integer()? != 0 {
                    let explicit_font = self.arg(call, 4)?.integer()?;
                    let font = if explicit_font == 0 {
                        self.font
                    } else {
                        Some(explicit_font)
                    };
                    let name = self
                        .decode_text(self.application_options.font_name())
                        .into_owned();
                    if let Some(font) = font.and_then(|id| self.fonts.get_mut(&id)) {
                        font.family = name;
                        font.bold = self.application_options.get(2, 4).unwrap_or_default() != 0;
                        font.effect = (
                            self.application_options.get(2, 3).unwrap_or_default() as u8,
                            2,
                            2,
                        );
                        if self.application_options.get(2, 2).unwrap_or(0) == 0 {
                            font.gradient = None;
                        }
                    }
                }
                if self.arg(call, 6)?.integer()? != 0 {
                    let limit = self.application_options.get(1, 5).unwrap_or(0) as usize;
                    self.backlog_limit = if limit == 0 { 0 } else { limit + 1 };
                }
                if consume {
                    self.settings_changed = false;
                }
                i32::from(changed)
            }
            19 => {
                let nested = self.low_priority_requests != 0;
                self.low_priority_requests = self.low_priority_requests.saturating_add(1);
                i32::from(nested)
            }
            20 => {
                let previous = self.low_priority_requests;
                if previous > 0 {
                    self.low_priority_requests -= 1;
                }
                previous
            }
            23 => {
                self.active_host_option = self.arg(call, 0)?.integer()? & 1 != 0;
                0
            }
            24 => {
                let command = self.arg(call, 0)?.integer()?;
                if command == 3 {
                    self.game.vm_mut().finish();
                    1
                } else {
                    // The other commands dispatch dialogs owned by the Win32 window.
                    0
                }
            }
            // This reports an active native window-menu command. The core's
            // canvas and script-owned controls do not run those commands.
            25 => 0,
            26..=29 => {
                let persistence = self.arg(call, 0)?.integer()? as u8;
                let storage = self.arg(call, 1)?.integer()? as u8;
                self.game
                    .vm_mut()
                    .change_variables(call.function, persistence, storage)?;
                0
            }
            34 | 489 => {
                if let Some(name) = self.save_filename(call)? {
                    if call.function == 34 {
                        self.global_save = Some(name);
                    } else {
                        self.text_log_file = Some(name);
                    }
                }
                0
            }
            490 => self.read_text_log()?,
            491 => {
                if self.dialogue.annotations_enabled {
                    let address = self.arg(call, 0)?.address()?;
                    self.dialogue.annotations = crate::dialogue::read_annotations(
                        self.game.vm(),
                        address,
                        self.dialogue.annotation_capacity,
                        self.text_encoding,
                    )?;
                    self.dialogue.annotation_key = self
                        .game
                        .vm()
                        .byte_tail(address)?
                        .and_then(|bytes| bytes.first_chunk::<3>())
                        .map(|&[kind, low, high]| (kind, u16::from_le_bytes([low, high])));
                    self.dialogue.annotation_selection = 0;
                    self.menu
                        .bind_text_list_annotations(self.dialogue.annotation_key);
                    0
                } else {
                    1
                }
            }
            492 => {
                self.dialogue.annotations_enabled = self.arg(call, 0)?.integer()? as u8 != 0;
                0
            }
            493 => {
                self.dialogue.annotations.clear();
                self.dialogue.annotation_key = None;
                self.menu.bind_text_list_annotations(None);
                self.dialogue.annotation_capacity = self.arg(call, 0)?.integer()?.max(0) as usize;
                1
            }
            494 => {
                self.dialogue.annotation_key = None;
                self.menu.bind_text_list_annotations(None);
                0
            }
            495 => {
                if !self.dialogue.annotations_enabled {
                    -1
                } else if let Word::Address(address) = self.arg(call, 0)? {
                    if self.dialogue.annotations.len() >= self.dialogue.annotation_capacity {
                        2
                    } else {
                        let label = self.game.vm().text(address)?;
                        if label.len() >= 253 {
                            3
                        } else {
                            self.dialogue
                                .annotations
                                .push(crate::dialogue::annotation(&label, 0)?);
                            0
                        }
                    }
                } else {
                    1
                }
            }
            496 => {
                self.dialogue.annotations.clear();
                self.dialogue.annotation_key = None;
                self.menu.bind_text_list_annotations(None);
                0
            }
            497 => self.dialogue.annotations.len() as i32,
            498 => self.draw_annotation(call)?,
            499 => self.copy_annotation(call)?,
            500 => return self.annotation_address(call).map(Some),
            501 => {
                let choice = self.arg(call, 0)?.integer()? as u8;
                self.dialogue.annotation_selection = choice;
                if let Some(key) = self.dialogue.choice_key(choice) {
                    self.dialogue.choices_read.insert(key);
                }
                1
            }
            502 => {
                if self.dialogue.annotations.is_empty() {
                    255
                } else {
                    i32::from(self.dialogue.annotation_selection)
                }
            }
            503 => {
                let choice = self.arg(call, 0)?.integer()? as u8;
                i32::from(
                    self.dialogue
                        .choice_key(choice)
                        .is_some_and(|key| self.dialogue.choices_read.contains(&key)),
                )
            }
            35 => self.write_global_save()?,
            36 => self.read_global_save()?,
            39 => {
                if self.arg(call, 0)?.integer()? == 0 {
                    0
                } else {
                    let slot_label = |index| match self.arg(call, index)? {
                        Word::Integer(0) => Ok(None),
                        pointer => pointer.address().map(Some),
                    };
                    self.slot_labels = [slot_label(1)?, slot_label(2)?];
                    self.game.vm_mut().clear_slot_variables();
                    1
                }
            }
            40 => {
                if let Word::Address(address) = self.arg(call, 0)? {
                    self.game.vm_mut().register_slot_variable(address);
                    1
                } else {
                    0
                }
            }
            41 => {
                let surface = self.arg(call, 0)?.integer()?;
                self.thumbnail_surface = (surface != 0).then_some(surface);
                1
            }
            42 => self.write_slot(call)?,
            43 => self.read_slot(call)?,
            44 => self.describe_slot(call)?,
            45 => self.preview_slot(call)?,
            49 => {
                let include_history = self.application_options.get(1, 6).unwrap_or(0) != 0
                    && self.application_options.get(6, 7).unwrap_or(0) == 0;
                self.slot_checkpoint = Some(self.game.vm_mut().encode_slot(
                    &self.slot_history,
                    include_history,
                    None,
                )?);
                0
            }
            50 => {
                if let Some(bytes) = self.slot_checkpoint.take() {
                    if self.restore_slot_state(&bytes).is_ok() {
                        0
                    } else {
                        2
                    }
                } else {
                    -1
                }
            }
            51 => i32::from(self.slot_checkpoint.is_some()),
            52 => {
                self.slot_checkpoint = None;
                0
            }
            53 => {
                let target = self.arg(call, 0)?.integer()?;
                let mode = self.arg(call, 1)?.integer()? as u8;
                if target == 0 || mode > 1 {
                    return Ok(Some(Word::Integer(0)));
                }
                let settings = crate::effects::SpritePulseSettings {
                    duration: self.arg(call, 2)?.integer()? as u32,
                    scale: self.effect_number(call, 3)?,
                    centered: mode == 1,
                    ease: self.effect_number(call, 4)?,
                };
                self.effects.start_sprite_pulse(
                    target,
                    settings,
                    self.timers.now(),
                    &mut self.graphics,
                )?
            }
            54 => 0,
            55 => {
                if self.arg(call, 0)?.integer()? == 0 {
                    let index = self.arg(call, 1)?.integer()? as usize;
                    if index < 2 {
                        self.thumbnail_conditions[index] = match self.arg(call, 2)? {
                            Word::Integer(0) => None,
                            word => Some(word.address()?),
                        };
                    }
                }
                0
            }
            57..=63 => return self.string_call(call).map(Some),
            67 => (self.effect_number(call, 0)? as f32).to_bits() as i32,
            76..=78 => {
                let destination = self.arg(call, 0)?;
                let value = self.arg(call, 1)?;
                if destination == Word::Integer(0)
                    || (call.function == 78 && value == Word::Integer(0))
                {
                    0
                } else {
                    let size = self.arg(call, 2)?.integer()? as u32;
                    let vm = self.game.vm_mut();
                    if call.function == 78 {
                        vm.copy_memory(destination.address()?, value.address()?, size)?;
                    } else {
                        vm.fill_memory(destination.address()?, value, size, call.function == 76)?;
                    }
                    1
                }
            }
            102 if self.arg(call, 0)?.integer()? == 1 && self.arg(call, 1)?.integer()? == 1 => {
                let first = self.arg(call, 2)?.integer()? as u32;
                let second = self.arg(call, 3)?.integer()? as u32;
                self.archive_key = Some((first, second));
                0
            }
            103 => self.mount_archive(call)?,
            65 => {
                let destination = self.arg(call, 0)?.address()?;
                let bytes = self.format_text(call, 1)?;
                self.game.vm_mut().write_text(destination, &bytes)?;
                0
            }
            68 | 69 => {
                if let Word::Address(address) = self.arg(call, 0)? {
                    self.change_text_case(address, call.function == 68)?;
                }
                0
            }
            82 => self.random.below(self.arg(call, 0)?.integer()? as u32),
            83 => {
                self.random = crate::random::Random::seeded(self.arg(call, 0)?.integer()?);
                0
            }
            163 | 164 => {
                let bytes = self.game.vm().text(self.arg(call, 0)?.address()?)?;
                let family = self
                    .text_encoding
                    .decode_without_bom_handling_and_without_replacement(&bytes)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen font name")
                    })?;
                let size = self.arg(call, 1)?.integer()? as u16;
                if size == 0 {
                    return Ok(Some(Word::Integer(0)));
                }
                let id = self.graphics.next_surface()?;
                self.fonts.insert(
                    id,
                    Font {
                        family: family.into_owned(),
                        size,
                        line_spacing: 0,
                        foreground: [255; 3],
                        shadow: [0; 3],
                        effect: (0, 0, 0),
                        glyph_capacity: 512,
                        gradient: None,
                        bold: false,
                        italic: false,
                    },
                );
                id
            }
            166 => {
                let id = self.arg(call, 0)?.integer()?;
                if self.fonts.contains_key(&id) {
                    self.font = Some(id);
                    1
                } else {
                    0
                }
            }
            167 | 168 => {
                let id = self.arg(call, 0)?.integer()?;
                if !self.fonts.contains_key(&id) {
                    return Ok(Some(Word::Integer(-1)));
                }
                let value = self.arg(call, 1)?;
                if call.function == 167 {
                    let bytes = self.game.vm().text(value.address()?)?;
                    let family = self.decode_text(&bytes).into_owned();
                    self.fonts.get_mut(&id).unwrap().family = family;
                } else {
                    let size = u16::try_from(value.integer()?.unsigned_abs()).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen font size")
                    })?;
                    self.fonts.get_mut(&id).unwrap().size = size.max(1);
                }
                0
            }
            169 => {
                let id = self.arg(call, 0)?.integer()?;
                let bold = self.arg(call, 1)?.integer()? != 0;
                let italic = self.arg(call, 2)?.integer()? != 0;
                let Some(font) = self.fonts.get_mut(&id) else {
                    return Ok(Some(Word::Integer(-1)));
                };
                font.bold = bold;
                font.italic = italic;
                0
            }
            172..=174 | 176 => {
                let id = self.arg(call, 0)?.integer()?;
                if !self.fonts.contains_key(&id) {
                    return Ok(Some(Word::Integer(0)));
                }
                let first = self.arg(call, 1)?.integer()?;
                if call.function == 176 {
                    self.fonts.get_mut(&id).unwrap().line_spacing = first;
                } else {
                    let second = self.arg(call, 2)?.integer()?;
                    let third = self.arg(call, 3)?.integer()?;
                    let font = self.fonts.get_mut(&id).unwrap();
                    match call.function {
                        172 => font.foreground = [first as u8, second as u8, third as u8],
                        173 => font.effect = (first as u8, second, third),
                        174 => font.shadow = [first as u8, second as u8, third as u8],
                        _ => unreachable!(),
                    }
                }
                1
            }
            179 => {
                let id = self.arg(call, 0)?.integer()?;
                let selector = self.arg(call, 1)?.integer()?;
                if let Some(font) = self.fonts.get_mut(&id) {
                    font.glyph_capacity = selector as u16;
                    1
                } else {
                    0
                }
            }
            181 => {
                let id = self.arg(call, 0)?.integer()?;
                let on = self.arg(call, 1)?.integer()? as u8 != 0;
                let start = self.arg(call, 2)?.integer()? as u32;
                let end = self.arg(call, 3)?.integer()? as u32;
                if let Some(font) = self.fonts.get_mut(&id) {
                    font.gradient = on.then_some([packed_rgb(start), packed_rgb(end)]);
                    0
                } else if id == 0 {
                    -1
                } else {
                    -2
                }
            }
            183 | 184 => self.measure_text(call)?,
            185..=188 => self.draw_text(call)?,
            196 => {
                self.graphics.clear();
                0
            }
            197 => {
                let group = self.arg(call, 0)?.integer()?;
                self.graphics.select_group(group)
            }
            198 => {
                let group = self.arg(call, 0)?.integer()?;
                let flags = self.arg(call, 1)?.integer()?;
                if flags & 1 != 0 {
                    self.graphics.release_group(group, flags & 2 != 0);
                }
                0
            }
            // No original file-buffer cache is enabled: LPK members are read by range.
            200 => 1,
            207 => {
                let task = self.arg(call, 0)?.integer()?;
                self.graphics
                    .update_transitions(self.timers.now(), &mut self.random);
                self.animations
                    .tick(self.timers.now(), &mut self.graphics)?;
                self.strips
                    .tick(self.timers.now(), &mut self.graphics, &mut self.random);
                self.effects
                    .tick(self.timers.now(), &mut self.graphics, &mut self.random);
                self.graphics.present_display()?;
                i32::from(
                    self.animations.status(task) != 0
                        || self.strips.playing(task)
                        || self.effects.playing(task),
                )
            }
            208 => {
                let pointer = self.arg(call, 0)?.address()?;
                let task = self.game.vm().read(pointer)?.integer()?;
                self.game.vm_mut().write(pointer, Word::Integer(0))?;
                i32::from(
                    self.animations.remove_player(task, &mut self.graphics)
                        || self.strips.stop(task, &mut self.graphics)
                        || self.effects.stop(task, &mut self.graphics),
                )
            }
            209 => {
                let task_type = self.arg(call, 0)?.integer()?;
                self.animations.stop_tasks(task_type, &mut self.graphics);
                self.effects
                    .stop_sprite_pulses(task_type, &mut self.graphics);
                if task_type == 13001 {
                    self.strips.stop_all(&mut self.graphics);
                }
                if task_type == 14000 {
                    self.effects.stop_background(&mut self.graphics);
                }
                if task_type == 14001 || task_type < 0 {
                    self.effects.stop_sprites(&mut self.graphics);
                }
                if task_type == 14003 || task_type < 0 {
                    self.effects.stop_scene_effects(&mut self.graphics);
                }
                0
            }
            210 | 212 => {
                let task = self.arg(call, 0)?.integer()?;
                let now = self.timers.now();
                let paused = call.function == 210;
                i32::from(
                    self.animations.pause_player(task, now, paused)
                        || self.strips.pause(task, now, paused)
                        || self.effects.pause(task, now, paused),
                )
            }
            211 | 213 => {
                let kind = self.arg(call, 0)?.integer()?;
                let now = self.timers.now();
                let paused = call.function == 211;
                self.animations.pause_tasks(kind, now, paused);
                self.effects.pause_sprite_pulses(kind, now, paused);
                if kind < 0 || kind == 13001 {
                    self.strips.pause_all(now, paused);
                }
                if kind < 0 || kind == 14000 {
                    self.effects.pause_background(now, paused);
                }
                if kind < 0 || kind == 14001 {
                    self.effects.pause_sprites(now, paused);
                }
                if kind < 0 || kind == 14003 {
                    self.effects.pause_scene_effects(now, paused);
                }
                0
            }
            214 => {
                let target = self.arg(call, 0)?.integer()?;
                if target == 0 {
                    return Ok(Some(Word::Integer(0)));
                }
                let effect = self.resource_name(call, 1)?;
                if matches!(effect.as_str(), "light" | "paper") {
                    let kind = if effect == "paper" {
                        crate::effects::ParticleKind::Paper
                    } else {
                        crate::effects::ParticleKind::Light
                    };
                    return Ok(Some(Word::Integer(
                        self.start_particles(call, target, kind)?,
                    )));
                }
                if effect == "bg_blur" {
                    return Ok(Some(Word::Integer(
                        self.start_background_transform(call, target)?,
                    )));
                }
                if effect == "bg_rot" {
                    let pose = crate::graphics::BackgroundRotation {
                        offset: [self.arg(call, 2)?.integer()?, self.arg(call, 3)?.integer()?],
                        turns: self.arg(call, 4)?.integer()?,
                        scale: self.effect_number(call, 5)?,
                    };
                    let ease = self.effect_number(call, 6)?;
                    let duration = self.arg(call, 7)?.integer()? as u32;
                    return Ok(Some(Word::Integer(
                        self.effects.start_background_rotation(
                            target,
                            pose,
                            duration,
                            ease,
                            self.timers.now(),
                            &mut self.graphics,
                        )?,
                    )));
                }
                if effect == "bg_zoom" {
                    let settings = crate::effects::ZoomSettings {
                        center: [self.arg(call, 2)?.integer()?, self.arg(call, 3)?.integer()?],
                        scale: self.effect_number(call, 4)?,
                        ease: self.effect_number(call, 5)?,
                        duration: self.arg(call, 6)?.integer()? as u32,
                    };
                    return Ok(Some(Word::Integer(self.effects.start_zoom(
                        target,
                        settings,
                        self.timers.now(),
                        &mut self.graphics,
                    )?)));
                }
                if effect == "sp_move" {
                    let texture = self.arg(call, 2)?.integer()?;
                    let path = self.resource_name(call, 3)?;
                    let duration = self.arg(call, 4)?.integer()? as u32;
                    return Ok(Some(Word::Integer(self.effects.start_sprite(
                        texture,
                        &path,
                        duration,
                        self.timers.now(),
                        &mut self.graphics,
                    )?)));
                }
                if effect == "noise" {
                    let settings = crate::effects::NoiseSettings {
                        opacity: self.arg(call, 2)?.integer()? as u8,
                        priority: self.arg(call, 3)?.integer()? as u32,
                        visible: [
                            self.arg(call, 4)?.integer()? != 0,
                            self.arg(call, 5)?.integer()? != 0,
                        ],
                    };
                    let bytes = self
                        .game
                        .assets_mut()
                        .read("sys/noise.elg", 64 * 1024 * 1024)?;
                    let id = self.graphics.load(&bytes)?;
                    let sample = self.graphics.surface(id)?.clone();
                    self.graphics.free(id);
                    return Ok(Some(Word::Integer(self.effects.start_noise(
                        target,
                        sample,
                        settings,
                        self.timers.now(),
                        &mut self.graphics,
                        &mut self.random,
                    )?)));
                }
                if effect == "quake" {
                    let end_gain = self
                        .resource_name(call, 6)?
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|gain| gain.is_finite())
                        .unwrap_or(0.0);
                    let settings = crate::effects::QuakeSettings {
                        duration: self.arg(call, 2)?.integer()? as u32,
                        frequency: self.arg(call, 3)?.integer()?,
                        amplitude: [self.arg(call, 4)?.integer()?, self.arg(call, 5)?.integer()?],
                        end_gain,
                        fill_edges: self.arg(call, 7)?.integer()? != 0,
                        primary: self.arg(call, 8)?.integer()? != 0,
                        overlay: self.arg(call, 9)?.integer()? != 0,
                    };
                    return Ok(Some(Word::Integer(self.effects.start_quake(
                        target,
                        settings,
                        self.timers.now(),
                        &mut self.graphics,
                    )?)));
                }
                if effect == "bg_haze" {
                    let spec = self.resource_name(call, 2)?;
                    let samples = self.arg(call, 3)?.integer()? as u32;
                    let timing = crate::effects::HazeTiming {
                        period: self.arg(call, 4)?.integer()? as u32,
                        sustain: self.arg(call, 5)?.integer()? as u32,
                        attack: self.arg(call, 6)?.integer()? as u32,
                        decay: self.arg(call, 7)?.integer()? as u32,
                        fill_edges: self.arg(call, 8)?.integer()? != 0,
                    };
                    return Ok(Some(Word::Integer(self.effects.start_haze(
                        target,
                        &spec,
                        samples,
                        timing,
                        self.timers.now(),
                        &mut self.graphics,
                    )?)));
                }
                if effect != "bg_move" {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!("unsupported Lucifen scene effect {effect}"),
                    ));
                }
                let mode = self.arg(call, 4)?.integer()?;
                if mode != 0xffff && (mode != 0 || self.arg(call, 5)?.integer()? != 0) {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "unsupported Lucifen background capture mode",
                    ));
                }
                let path = self.resource_name(call, 2)?;
                let duration = self.arg(call, 3)?.integer()? as u32;
                self.effects.start_background(
                    target,
                    &path,
                    duration,
                    self.timers.now(),
                    &mut self.graphics,
                )?
            }
            215 => {
                let handle = self.arg(call, 0)?.integer()?;
                if self.effects.is_noise(handle) {
                    let visible = [
                        self.arg(call, 1)?.integer()? != 0,
                        self.arg(call, 2)?.integer()? != 0,
                    ];
                    self.effects
                        .configure_noise(handle, visible, &mut self.graphics)
                } else if self.effects.is_particles(handle) {
                    let visible = if self.arg(call, 1)?.integer()? != 0 {
                        Some([
                            self.arg(call, 2)?.integer()? != 0,
                            self.arg(call, 3)?.integer()? != 0,
                        ])
                    } else {
                        None
                    };
                    self.effects
                        .configure_particles(handle, visible, &mut self.graphics)
                        .unwrap_or(-1)
                } else if self.effects.playing(handle) {
                    -2
                } else {
                    -1
                }
            }
            216 => {
                let id = self.arg(call, 0)?.integer()?;
                if let Some(values) = self
                    .effects
                    .sprite_end(id)
                    .or_else(|| self.effects.zoom_end(id))
                {
                    for (index, value) in values.into_iter().enumerate() {
                        if let Word::Address(address) = self.arg(call, index + 1)? {
                            self.game.vm_mut().write(address, Word::Integer(value))?;
                        }
                    }
                    0
                } else if let Some(position) = self.effects.end(id) {
                    for (index, value) in position.into_iter().enumerate() {
                        if let Word::Address(address) = self.arg(call, index + 1)? {
                            self.game.vm_mut().write(address, Word::Integer(value))?;
                        }
                    }
                    0
                } else if let Some(status) = self.effects.fade_haze(id, self.timers.now()) {
                    status
                } else if id == 0 {
                    -1
                } else if self.effects.playing(id) {
                    -2
                } else {
                    return Ok(None);
                }
            }
            217 => {
                let name = self.game.vm().text(self.arg(call, 0)?.address()?)?;
                let target = self.arg(call, 1)?.integer()?;
                match name.as_slice() {
                    b"sp_move" if target == 0 => {
                        self.graphics.sprite_move_priorities =
                            Some([self.arg(call, 2)?.integer()?, self.arg(call, 3)?.integer()?]);
                        0
                    }
                    b"sp_move" => self.position_sprite(call, target)?,
                    b"bg_move" => {
                        let path = self.resource_name(call, 2)?;
                        if let Some(position) = crate::effects::Effects::position_background(
                            target,
                            &path,
                            &mut self.graphics,
                        )? {
                            for (index, value) in position.into_iter().enumerate() {
                                if let Word::Address(output) = self.arg(call, index + 3)? {
                                    self.game.vm_mut().write(output, Word::Integer(value))?;
                                }
                            }
                            0
                        } else {
                            -1
                        }
                    }
                    b"bg_zoom" => {
                        let center = [self.arg(call, 2)?.integer()?, self.arg(call, 3)?.integer()?];
                        let scale = self.effect_number(call, 4)?;
                        if let Some(window) = crate::effects::Effects::zoom_window(
                            target,
                            center,
                            scale,
                            &self.graphics,
                        ) {
                            self.graphics.set_background_window(target, window);
                            for (index, value) in window.into_iter().enumerate() {
                                if let Word::Address(output) = self.arg(call, index + 5)? {
                                    self.game.vm_mut().write(output, Word::Integer(value))?;
                                }
                            }
                            0
                        } else {
                            -1
                        }
                    }
                    _ => -1,
                }
            }
            232 => self.rasterize_text(call)?,
            233 => self.graphics.clone_bitmap(self.arg(call, 0)?.integer()?)?,
            227 | 234 | 235 => {
                let width = self.arg(call, 0)?.integer()? as u32;
                let height = self.arg(call, 1)?.integer()? as u32;
                let depth = self.arg(call, 2)?.integer()? as u8;
                if depth != 32 {
                    return Ok(None);
                }
                self.graphics.create(width, height)?
            }
            // Both native allocation paths use RGBA surfaces in the CPU renderer.
            228 | 229 => {
                if self.arg(call, 0)? == Word::Integer(0) {
                    return Ok(Some(Word::Integer(0)));
                }
                let name = self.resource_name(call, 0)?;
                let asset = match self.game.assets_mut().read(&name, 64 * 1024 * 1024) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        return Ok(Some(Word::Integer(0)));
                    }
                    Err(error) => return Err(error),
                };
                self.graphics.load(&asset)?
            }
            237 => self.graphics.display(),
            238 => {
                let id = self.arg(call, 0)?.integer()?;
                i32::from(!self.graphics.select_display(id))
            }
            239 => {
                let output = self.arg(call, 0)?.address()?;
                let id = self.game.vm().read(output)?.integer()?;
                if id == 0 {
                    1
                } else if self.graphics.surface(id).is_err() {
                    2
                } else {
                    self.graphics.free(id);
                    self.game.vm_mut().write(output, Word::Integer(0))?;
                    0
                }
            }
            240 | 241 | 245 | 246 => {
                let id = self.arg(call, 0)?.integer()?;
                self.graphics
                    .surface(id)
                    .map_or(0, |surface| match call.function {
                        240 => surface.width as i32,
                        241 => surface.height as i32,
                        245 => i32::from(surface.origin.0),
                        _ => i32::from(surface.origin.1),
                    })
            }
            249 => {
                let target = self.arg(call, 0)?.integer()?;
                let source = self.arg(call, 1)?.integer()?;
                self.graphics.set_transition_source(target, source)
            }
            250 => i32::from(self.graphics.exchange_transition_scene(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()? as u8,
            )),
            251 => {
                let name = self.resource_name(call, 1)?;
                let (rule, quake, transform) = match name.to_ascii_lowercase().as_str() {
                    "cross" => (None, None, None),
                    "addsub" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::AddSubtract {
                            flags: self.arg(call, 5)?.integer()? as u8,
                        }),
                    ),
                    "scroll" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::Scroll {
                            direction: self.arg(call, 5)?.integer()? as u8,
                            style: self.arg(call, 6)?.integer()? as u8,
                        }),
                    ),
                    "wave" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::Wave {
                            mode: self.arg(call, 5)?.integer()?,
                            amplitude: self.arg(call, 6)?.integer()?,
                            frequency: self.arg(call, 7)?.integer()?,
                            border: [
                                packed_rgb(self.arg(call, 8)?.integer()? as u32),
                                packed_rgb(self.arg(call, 9)?.integer()? as u32),
                            ],
                        }),
                    ),
                    "mosaic" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::Mosaic {
                            alignment: match self.arg(call, 5)?.integer()? as u8 {
                                alignment @ 1..=4 => alignment,
                                _ => 0,
                            },
                            size: self.arg(call, 6)?.integer()?.max(1) as u32,
                            fit_aspect: self.arg(call, 7)?.integer()? != 0,
                        }),
                    ),
                    "zoomflip" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::ZoomFlip {
                            mode: (self.arg(call, 5)?.integer()? as u8).min(6),
                            sampling: self.arg(call, 6)?.integer()? as u8,
                        }),
                    ),
                    "rotaxis" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::RotateAxis {
                            direction: self.arg(call, 5)?.integer()? as u8,
                        }),
                    ),
                    "rotcenter" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::RotateCenter {
                            style: self.arg(call, 5)?.integer()? as u8,
                            sampling: self.arg(call, 6)?.integer()? as u8,
                        }),
                    ),
                    "hblur" => (
                        None,
                        None,
                        Some(crate::graphics::TransitionTransform::HorizontalBlur {
                            cross: self.arg(call, 5)?.integer()? as u8 != 0,
                            radius: (self.arg(call, 6)?.integer()? as u8).clamp(1, 127),
                        }),
                    ),
                    "rule" => (
                        Some((self.arg(call, 5)?.integer()?, self.arg(call, 6)?.integer()?)),
                        None,
                        None,
                    ),
                    "quake" => {
                        let end_gain = self
                            .resource_name(call, 8)?
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .filter(|gain| gain.is_finite())
                            .unwrap_or(0.0);
                        let quake = crate::graphics::TransitionQuake {
                            amplitude: [
                                self.arg(call, 5)?.integer()?,
                                self.arg(call, 6)?.integer()?,
                            ],
                            end_gain,
                            fill_edges: self.arg(call, 7)?.integer()? != 0,
                        };
                        (None, Some(quake), None)
                    }
                    "trans" => {
                        let operation = self.arg(call, 5)?.integer()? as u8;
                        let reverse = self.arg(call, 6)?.integer()? != 0;
                        let transform = match operation {
                            86 => crate::graphics::TransitionTransform::VerticalSplit { reverse },
                            _ => {
                                return Err(io::Error::new(
                                    io::ErrorKind::Unsupported,
                                    format!("unsupported Lucifen transform transition {operation}"),
                                ));
                            }
                        };
                        (None, None, Some(transform))
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            format!("unsupported Lucifen render effect {name}"),
                        ));
                    }
                };
                let target = self.arg(call, 0)?.integer()?;
                let mode = self.arg(call, 2)?.integer()? as u8;
                let commit = self.arg(call, 3)?.integer()? != 0;
                let mut duration = self.arg(call, 4)?.integer()? as u32;
                if matches!(
                    transform,
                    Some(crate::graphics::TransitionTransform::Wave { .. })
                ) {
                    duration = duration.max(2);
                }
                self.graphics.begin_transition(
                    target,
                    crate::graphics::SceneEffect {
                        duration,
                        rule,
                        mode,
                        commit,
                        quake,
                        transform,
                    },
                    self.timers.now(),
                )?
            }
            252 => {
                let target = self.arg(call, 0)?.integer()?;
                self.graphics.finish_transition(target);
                i32::from(self.graphics.surface(target).is_ok())
            }
            253 => i32::from(
                self.graphics
                    .transition_active(self.arg(call, 0)?.integer()?),
            ),
            254 => self
                .graphics
                .transition_buffer(self.arg(call, 0)?.integer()?),
            315 => self.load_bitmap_into(call)?,
            318 => {
                let id = self.arg(call, 0)?.integer()?;
                let [blue, green, red, alpha] = self.arg(call, 1)?.integer()?.to_le_bytes();
                let left = self.arg(call, 2)?.integer()?;
                let within = if left == 0xffff {
                    None
                } else {
                    Some([
                        left,
                        self.arg(call, 3)?.integer()?,
                        self.arg(call, 4)?.integer()?,
                        self.arg(call, 5)?.integer()?,
                    ])
                };
                i32::from(self.graphics.fill(id, within, [red, green, blue, alpha]))
            }
            328 => self.copy_bitmap(call)?,
            346 => {
                let target = self.arg(call, 0)?.integer()?;
                let level = self.arg(call, 1)?.integer()? as u16;
                let region = self.bitmap_region(call, 2)?;
                self.graphics.set_bitmap_brightness(target, level, region)
            }
            332 | 362 => {
                let target = self.arg(call, 0)?.integer()?;
                let source = self.arg(call, 1)?.integer()?;
                let (arg, blend) = if call.function == 332 {
                    (2, crate::graphics::BitmapBlend::Color)
                } else {
                    (
                        3,
                        crate::graphics::BitmapBlend::Alpha(self.arg(call, 2)?.integer()? as u8),
                    )
                };
                let x = self.arg(call, arg)?.integer()?;
                let (position, region) = if x == 0xffff {
                    ([0; 2], None)
                } else {
                    (
                        [x, self.arg(call, arg + 1)?.integer()?],
                        self.bitmap_region(call, arg + 2)?,
                    )
                };
                self.graphics
                    .blend_bitmap(target, source, position, region, blend)
            }
            373 => {
                let target = self.arg(call, 0)?.integer()?;
                let region = self.bitmap_region(call, 1)?;
                self.graphics.grayscale_bitmap(target, region)
            }
            374 => {
                let target = self.arg(call, 0)?.integer()?;
                let mut gain = [0; 3];
                let mut offset = [0; 3];
                for (channel, (gain, offset)) in gain.iter_mut().zip(&mut offset).enumerate() {
                    *gain = i32::from(self.arg(call, channel + 1)?.integer()? as u16);
                    *offset = i32::from(self.arg(call, channel + 4)?.integer()? as i16);
                }
                let region = self.bitmap_region(call, 7)?;
                self.graphics
                    .adjust_bitmap_colors(target, region, gain, offset)
            }
            375 => {
                let target = self.arg(call, 0)?.integer()?;
                let mode = self.arg(call, 1)?.integer()? as u8;
                let region = self.bitmap_region(call, 2)?;
                self.graphics.invert_bitmap_colors(target, region, mode)
            }
            386 => {
                let variant = self.arg(call, 2)?.integer()?;
                let mode = self.arg(call, 3)?.integer()?;
                let unobstructed = self.arg(call, 6)?.integer()? == 0xffff;
                if variant != 0 || mode != 2 || !unobstructed {
                    return Ok(None);
                }
                let destination = self.arg(call, 0)?.integer()?;
                let source = self.arg(call, 1)?.integer()?;
                let opacity = self.arg(call, 4)?.integer()? as u8;
                self.graphics
                    .composite(destination, source, 0, 0, opacity)?;
                0
            }
            389 => {
                let target = self.arg(call, 0)?.integer()?;
                let radius = [
                    self.arg(call, 1)?.integer()?.unsigned_abs(),
                    self.arg(call, 2)?.integer()?.unsigned_abs(),
                ];
                let alpha = self.arg(call, 3)?.integer()? != 0;
                let left = self.arg(call, 4)?.integer()?;
                let region = if left == 0xffff {
                    None
                } else {
                    Some([
                        left,
                        self.arg(call, 5)?.integer()?,
                        self.arg(call, 6)?.integer()?,
                        self.arg(call, 7)?.integer()?,
                    ])
                };
                self.graphics.blur_bitmap(target, radius, region, alpha)
            }
            387 | 388 | 390 | 391 => {
                let target = self.arg(call, 0)?.integer()?;
                let left = self.arg(call, 1)?.integer()?;
                let region = if left == 0xffff {
                    None
                } else {
                    Some([
                        left,
                        self.arg(call, 2)?.integer()?,
                        self.arg(call, 3)?.integer()?,
                        self.arg(call, 4)?.integer()?,
                    ])
                };
                self.graphics
                    .flip_bitmap(target, region, matches!(call.function, 388 | 391))
            }
            396 => self.blend_bitmap_rule(call)?,
            397 => {
                let id = self.arg(call, 0)?.integer()?;
                let left = self.arg(call, 1)?.integer()?;
                let (rect, position) = if left == 0xffff {
                    (None, [0, 0])
                } else {
                    (
                        Some([
                            left,
                            self.arg(call, 2)?.integer()?,
                            self.arg(call, 3)?.integer()?,
                            self.arg(call, 4)?.integer()?,
                        ]),
                        [self.arg(call, 5)?.integer()?, self.arg(call, 6)?.integer()?],
                    )
                };
                self.graphics.texture_from_surface(id, rect, position)?
            }
            398 => self.graphics.clone_texture(self.arg(call, 0)?.integer()?)?,
            400 => {
                self.graphics.clear_textures();
                0
            }
            401 => {
                let group = self.arg(call, 0)?.integer()?;
                self.graphics.select_texture_group(group)
            }
            402 => {
                let group = self.arg(call, 0)?.integer()?;
                let keep_group = self.arg(call, 1)?.integer()? as u8 != 0;
                self.graphics.release_texture_group(group, keep_group);
                0
            }
            403 => {
                let address = self.arg(call, 0)?.address()?;
                let id = self.game.vm().read(address)?.integer()?;
                if self.graphics.free_texture(id) {
                    self.game.vm_mut().write(address, Word::Integer(0))?;
                    0
                } else {
                    1
                }
            }
            404 | 405 => {
                let surface = self.arg(call, 0)?.integer()?;
                let texture = self.arg(call, 1)?.integer()?;
                let priority = self.arg(call, 2)?.integer()?;
                let (priority, group, overlay) = if priority == -1 {
                    (0, 0, false)
                } else {
                    (
                        priority as u32,
                        self.arg(call, 3)?.integer()?,
                        self.arg(call, 4)?.integer()? != 0,
                    )
                };
                self.graphics.attach_layer(
                    surface,
                    texture,
                    priority,
                    group,
                    overlay,
                    call.function == 405,
                )
            }
            406 => {
                let surface = self.arg(call, 0)?.integer()?;
                let texture = self.arg(call, 1)?.integer()?;
                i32::from(self.graphics.remove_layer(surface, texture))
            }
            407 => {
                let surface = self.arg(call, 0)?.integer()?;
                let priority = self.arg(call, 1)?.integer()? as u32;
                let others = self.arg(call, 2)?.integer()? != 0;
                self.graphics.remove_layers(surface, priority, others);
                0
            }
            408 => {
                let surface = self.arg(call, 0)?.integer()?;
                let group = self.arg(call, 1)?.integer()?;
                let texture_group = self.arg(call, 2)?.integer()?;
                let others = self.arg(call, 3)?.integer()? != 0;
                self.graphics
                    .remove_layer_groups(surface, group, texture_group, others);
                0
            }
            409 | 410 => {
                let surface = self.arg(call, 0)?.integer()?;
                let texture = self.arg(call, 1)?.integer()?;
                let priority = self.arg(call, 2)?.integer()? as u32;
                let conditional = self.arg(call, 3)?.integer()? != 0;
                i32::from(self.graphics.move_layer(
                    surface,
                    texture,
                    priority,
                    conditional,
                    call.function == 410,
                ))
            }
            411 => {
                self.graphics.clear_layers(self.arg(call, 0)?.integer()?);
                0
            }
            412 => {
                let target = self.arg(call, 0)?.integer()?;
                let visible = self.arg(call, 1)?.integer()? != 0;
                let priority = self.arg(call, 2)?.integer()?;
                let layer_group = self.arg(call, 3)?.integer()?;
                let (texture_group, inverted) = if layer_group == 0xffff {
                    (-1, false)
                } else {
                    (
                        self.arg(call, 4)?.integer()?,
                        self.arg(call, 5)?.integer()? != 0,
                    )
                };
                self.graphics.set_layers_visible(
                    target,
                    visible,
                    priority,
                    if layer_group == 0xffff {
                        -1
                    } else {
                        layer_group
                    },
                    texture_group,
                    inverted,
                );
                0
            }
            413 => i32::from(self.graphics.set_background_image(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()?,
                [self.arg(call, 2)?.integer()?, self.arg(call, 3)?.integer()?],
                self.arg(call, 4)?.integer()? != 0,
            )?),
            414 => {
                let target = self.arg(call, 0)?.integer()?;
                let [blue, green, red, _] = self.arg(call, 1)?.integer()?.to_le_bytes();
                let mode = self.arg(call, 2)?.integer()? as u8;
                i32::from(self.graphics.set_background_color(
                    target,
                    [red, green, blue, 255],
                    mode,
                )?)
            }
            415 => self.graphics.move_background(
                self.arg(call, 0)?.integer()?,
                [self.arg(call, 1)?.integer()?, self.arg(call, 2)?.integer()?],
            )?,
            416 => self.graphics.stretch_background(
                self.arg(call, 0)?.integer()?,
                [
                    self.arg(call, 1)?.integer()?,
                    self.arg(call, 2)?.integer()?,
                    self.arg(call, 3)?.integer()?,
                    self.arg(call, 4)?.integer()?,
                ],
                self.bitmap_region(call, 5)?,
            ),
            417 => {
                let target = self.arg(call, 0)?.integer()?;
                let mode = self.arg(call, 1)?.integer()?;
                let point = if (0..=4).contains(&mode) {
                    [0; 2]
                } else {
                    [self.arg(call, 2)?.integer()?, self.arg(call, 3)?.integer()?]
                };
                i32::from(self.graphics.set_background_pivot(target, mode, point))
            }
            418 => {
                let target = self.arg(call, 0)?.integer()?;
                let scale = f32::from_bits(self.arg(call, 4)?.integer()? as u32);
                if !scale.is_finite() {
                    return Ok(Some(Word::Integer(0)));
                }
                let pose = crate::graphics::BackgroundRotation {
                    offset: [self.arg(call, 1)?.integer()?, self.arg(call, 2)?.integer()?],
                    turns: self.arg(call, 3)?.integer()?,
                    scale: f64::from(scale),
                };
                i32::from(self.graphics.rotate_background(target, pose))
            }
            419 => {
                let target = self.arg(call, 0)?.integer()?;
                let Some((bitmap, [x, y], rgb)) = self.graphics.background_state(target) else {
                    return Ok(Some(Word::Integer(0)));
                };
                for (index, value) in [bitmap, x, y].into_iter().enumerate() {
                    if let Word::Address(out) = self.arg(call, index + 1)? {
                        self.game.vm_mut().write(out, Word::Integer(value))?;
                    }
                }
                for (index, value) in rgb.into_iter().enumerate() {
                    if let Word::Address(out) = self.arg(call, index + 4)? {
                        self.game.vm_mut().write_data(out, &[value])?;
                    }
                }
                1
            }
            420 | 421 => {
                let valid = self
                    .graphics
                    .invalidate_scene(self.arg(call, 0)?.integer()?);
                if call.function == 420 {
                    if valid { 0 } else { -1 }
                } else {
                    i32::from(valid)
                }
            }
            422 => self.graphics.set_manual_refresh(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()? != 0,
            ),
            424 => self.graphics.capture_scene(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()?,
                self.arg(call, 2)?.integer()? as u8,
                self.arg(call, 3)?.integer()?,
                self.arg(call, 4)?.integer()? != 0,
            )?,
            425 => self.graphics.hide_layer_passes(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()? != 0,
                self.arg(call, 2)?.integer()? != 0,
            ),
            426 => self.graphics.hide_background_pass(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()? != 0,
            ),
            428 => {
                let id = self.arg(call, 0)?.integer()?;
                let visible = self.arg(call, 1)?.integer()? != 0;
                self.graphics.show_texture(id, visible);
                0
            }
            430 => i32::from(
                self.graphics
                    .invalidate_texture(self.arg(call, 0)?.integer()?),
            ),
            429 => {
                if self.graphics.is_layer(self.arg(call, 0)?.integer()?) {
                    2
                } else {
                    0
                }
            }
            432 | 433 => {
                let id = self.arg(call, 0)?.integer()?;
                let value = self.arg(call, 1)?.integer()? as u8;
                if call.function == 432 {
                    self.graphics.texture_blend(id, value)
                } else {
                    self.graphics.texture_opacity(id, value)
                }
            }
            434 | 435 => {
                let id = self.arg(call, 0)?.integer()?;
                let x = self.arg(call, 1)?.integer()?;
                let y = self.arg(call, 2)?.integer()?;
                i32::from(self.graphics.texture_move(id, x, y, call.function == 435))
            }
            436 => i32::from(self.graphics.hit_texture(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()?,
                self.arg(call, 2)?.integer()?,
                self.arg(call, 3)?.integer()? != 0,
            )),
            438 => {
                let id = self.arg(call, 0)?.integer()?;
                if let Some(bounds) = self.graphics.texture_bounds(id) {
                    for (index, value) in bounds.into_iter().enumerate() {
                        let out = self.arg(call, index + 1)?;
                        if let Word::Address(address) = out {
                            self.game.vm_mut().write(address, Word::Integer(value))?;
                        }
                    }
                    1
                } else {
                    0
                }
            }
            439 => {
                let target = self.arg(call, 0)?.integer()?;
                let texture = self.arg(call, 1)?.integer()?;
                self.graphics.draw_texture(texture, target)?
            }
            440 | 441 => self.move_sprite(call)?,
            449 => {
                let texture = self.arg(call, 0)?.integer()?;
                let x = self.arg(call, 1)?.integer()?;
                let y = self.arg(call, 2)?.integer()?;
                if self.graphics.move_texture_source(texture, [x, y]) {
                    0
                } else {
                    -1
                }
            }
            450 => i32::from(self.graphics.set_texture_sampling(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()? as u8,
            )),
            455 => {
                let id = self.arg(call, 0)?.integer()?;
                let source = self.arg(call, 1)?.integer()?;
                let left = self.arg(call, 2)?.integer()?;
                let placement = match left {
                    -1 => None,
                    0xffff => Some((None, [0, 0])),
                    _ => Some((
                        Some([
                            left,
                            self.arg(call, 3)?.integer()?,
                            self.arg(call, 4)?.integer()?,
                            self.arg(call, 5)?.integer()?,
                        ]),
                        [self.arg(call, 6)?.integer()?, self.arg(call, 7)?.integer()?],
                    )),
                };
                self.graphics.rebind_texture(id, source, placement)?
            }
            461 => {
                self.backlog_enabled = self.arg(call, 0)?.integer()? != 0;
                0
            }
            462 => self.open_dialogue(call)?,
            463 => {
                self.dialogue_enabled = self.arg(call, 0)?.integer()? as u8 != 0;
                0
            }
            464 => self.set_dialogue_destination(call)?,
            465 => self.dialogue_speaker(call, false)?,
            466 => self.dialogue_speaker(call, true)?,
            468 => i32::from(self.scan_dialogue(-1, true)? >= 0),
            469 => self.scan_dialogue(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()? as u8 != 0,
            )?,
            470 => i32::from(self.draw_dialogue()?),
            472 => {
                let output = self.arg(call, 0)?.address()?;
                if let Some(packet) = &self.dialogue.packet
                    && !packet.command.is_empty()
                {
                    self.game.vm_mut().write_text(output, &packet.command)?;
                    1
                } else {
                    0
                }
            }
            473 => self
                .dialogue
                .packet
                .as_ref()
                .map_or(0, |packet| packet.parameter),
            474 => i32::from(self.slot_history.position > 0),
            475 => i32::from(self.slot_history.position < self.slot_history.entry_count() - 1),
            476 => self.dialogue_summary(call)?,
            477 => self.slot_history.entry_count(),
            478 => self.slot_history.position,
            479..=484 => self.select_history(call)?,
            487 => {
                self.slot_history = crate::state::SlotHistory::default();
                0
            }
            504 | 507 => {
                let name_index = usize::from(call.function == 507);
                let flags = self.arg(call, name_index + 1)?.integer()?;
                if flags != 0 {
                    return Ok(None);
                }
                let name = self.resource_name(call, name_index)?;
                let data = self.game.assets_mut().read(&name, 128 * 1024 * 1024)?;
                if call.function == 504 {
                    let id = self.graphics.next_surface()?;
                    self.animations.insert(id, data)?;
                    id
                } else {
                    let id = self.arg(call, 0)?.integer()?;
                    self.animations.remove(id, &mut self.graphics);
                    self.animations.insert(id, data)?;
                    0
                }
            }
            505 => {
                let output = self.arg(call, 0)?.address()?;
                let id = self.game.vm().read(output)?.integer()?;
                if self.animations.remove(id, &mut self.graphics) {
                    self.game.vm_mut().write(output, Word::Integer(0))?;
                    0
                } else {
                    1
                }
            }
            506 => {
                self.animations.clear(&mut self.graphics);
                0
            }
            508 => {
                let layer = self.arg(call, 4)?.integer()?;
                let default = layer == -1;
                let request = crate::animation::Playback {
                    target: self.arg(call, 0)?.integer()?,
                    clip: self.arg(call, 1)?.integer()?,
                    sequence: self.arg(call, 2)?.integer()? as usize,
                    repeats: self.arg(call, 3)?.integer()?,
                    priority: if default { 0 } else { layer as u32 },
                    catch_up: default || self.arg(call, 5)?.integer()? != 0,
                    detach_on_end: default || self.arg(call, 6)?.integer()? != 0,
                };
                self.animations
                    .start(request, self.timers.now(), &mut self.graphics)?
            }
            509 => self.animations.status(self.arg(call, 0)?.integer()?),
            520 => {
                if self.arg(call, 0)? == Word::Integer(0) {
                    1
                } else {
                    let resource = self.resource_name(call, 0)?;
                    let first_command = self.arg(call, 1)?.integer()?;
                    let priority = self.arg(call, 2)?.integer()? as u32;
                    let color_keyed = self.arg(call, 3)?.integer()? != 0;
                    self.menu.load(
                        self.game.assets_mut(),
                        &mut self.graphics,
                        &resource,
                        first_command,
                        priority,
                        color_keyed,
                    )?
                }
            }
            521 => {
                self.menu.clear();
                0
            }
            522 => self.menu.controls.len() as i32,
            523 | 524 => self.read_menu_controls(call)?,
            525 => self.open_strip(call)?,
            526 | 527 => {
                let strip = self.arg(call, 0)?.integer()?;
                let target = self.arg(call, 1)?.integer()?;
                self.strips
                    .insert_redraw(strip, target, call.function == 526);
                0
            }
            528 => {
                if self.arg(call, 0)? == Word::Integer(0) {
                    1
                } else {
                    let address = self.arg(call, 0)?.address()?;
                    let id = self.game.vm().read(address)?.integer()?;
                    if self.strips.remove(id, &mut self.graphics) {
                        self.game.vm_mut().write(address, Word::Integer(0))?;
                        0
                    } else {
                        1
                    }
                }
            }
            529 => {
                self.strips.clear(&mut self.graphics);
                0
            }
            531 => self.strips.start(
                self.arg(call, 0)?.integer()?,
                crate::strip::StripPlayback {
                    repeats: self.arg(call, 1)?.integer()? as u8,
                    interval: self.arg(call, 2)?.integer()? as u32,
                    jitter: self.arg(call, 3)?.integer()? as u32,
                    delayed: self.arg(call, 4)?.integer()? != 0,
                },
                self.timers.now(),
                &mut self.graphics,
                &mut self.random,
            )?,
            534 => self.timers.now() as i32,
            536..=543 => {
                let slot = if call.function >= 542 {
                    0
                } else {
                    self.arg(call, 0)?.integer()? as u16
                };
                let value = if call.function == 539 {
                    self.arg(call, 1)?.integer()? as u32
                } else {
                    0
                };
                self.timers.control(call.function, slot, value)
            }
            544 | 545 => self
                .input
                .action(self.arg(call, 0)?.integer()?, call.function == 544),
            546 | 547 => self.input.first_action(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()?,
                call.function == 546,
            ),
            548 | 549 => {
                let action = self.arg(call, 0)?.integer()?;
                let device = self.arg(call, 1)?.integer()?;
                let code = self.arg(call, 2)?.integer()? as u8;
                self.input
                    .change_binding(action, device, code, call.function == 548)
            }
            551..=562 => {
                let offset = (call.function - 551) as usize;
                self.input
                    .action([4, 5, 0, 1, 2, 3][offset % 6], offset < 6)
            }
            563 | 564 => {
                let keys = if self.application_options.get(4, 0).unwrap_or(0) != 0 {
                    [0x1d, 0x9d]
                } else {
                    [0x2a, 0x36]
                };
                i32::from(
                    keys.into_iter()
                        .any(|key| self.input.key(key, call.function == 563)),
                )
            }
            565 | 566 => i32::from(
                self.input
                    .key(self.arg(call, 0)?.integer()? as u8, call.function == 565),
            ),
            574 => {
                self.input.cursor_visible = self.arg(call, 0)?.integer()? & 1 != 0;
                0
            }
            575 => {
                let x = self.arg(call, 0)?.address()?;
                let y = self.arg(call, 1)?.address()?;
                self.game
                    .vm_mut()
                    .write(x, Word::Integer(self.input.pointer[0]))?;
                self.game
                    .vm_mut()
                    .write(y, Word::Integer(self.input.pointer[1]))?;
                0
            }
            576 => {
                let x = self.arg(call, 0)?.integer()?;
                let y = self.arg(call, 1)?.integer()?;
                self.input.pointer =
                    if (x as u32) < self.system.width && (y as u32) < self.system.height {
                        [x, y]
                    } else {
                        [-1; 2]
                    };
                0
            }
            577 => self
                .input
                .edge_direction(self.system.width, self.system.height),
            578..=583 => {
                let offset = (call.function - 578) as usize;
                i32::from(self.input.mouse(offset % 3, offset < 3))
            }
            589 => {
                if self.arg(call, 0)?.integer()? != 0 {
                    return Ok(None);
                }
                if self.audio.is_none() {
                    let output = if self.null_audio {
                        shinki_emu_sdk::NativeAudioOutput::open_null()
                    } else {
                        shinki_emu_sdk::NativeAudioOutput::open(self.audio_stop.clone())
                    };
                    self.audio = Some(output.map_err(io::Error::other)?);
                }
                0
            }
            601..=605 => {
                let handle = self.arg(call, 0)?.integer()?;
                self.channels.control_handle(handle, call.function)
            }
            613 | 614 => self
                .channels
                .time_ms(self.arg(call, 0)?.integer()?, call.function == 613),
            615 => {
                if self.arg(call, 1)? != Word::Integer(0) || self.arg(call, 2)? != Word::Integer(0)
                {
                    return Ok(None);
                }
                i32::from(self.channels.handle_playing(self.arg(call, 0)?.integer()?))
            }
            616 => self
                .channels
                .set_handle_gain(self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?),
            650 | 655 | 657 => {
                let index = usize::from(call.function != 650);
                let mode = self.arg(call, index + 1)?.integer()?;
                let intro_name = if mode != 0xffff && self.arg(call, index + 2)? != Word::Integer(0)
                {
                    Some(self.resource_name(call, index + 2)?).filter(|name| !name.is_empty())
                } else {
                    None
                };
                let name = self.resource_name(call, index)?;
                if self.channels.music_is_playing(&name, intro_name.as_deref()) {
                    return Ok(Some(Word::Integer(-1)));
                }
                let bytes = match self.game.assets_mut().read(&name, 64 * 1024 * 1024) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        return Ok(Some(Word::Integer(2)));
                    }
                    Err(error) => return Err(error),
                };
                let intro = if let Some(name) = intro_name {
                    match self.game.assets_mut().read(&name, 64 * 1024 * 1024) {
                        Ok(bytes) => Some((name, bytes)),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            return Ok(Some(Word::Integer(2)));
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    None
                };
                let source = crate::sound::MusicSource {
                    name,
                    bytes: &bytes,
                    intro: intro
                        .as_ref()
                        .map(|(name, bytes)| (name.clone(), bytes.as_slice())),
                };
                let fade_ms = if call.function != 650 {
                    self.arg(call, 0)?.integer()?.max(0) as u32
                } else {
                    0
                };
                if call.function == 657 {
                    let explicit = mode != 0xffff && self.arg(call, 4)?.integer()? != 0xffff;
                    let (overlap, volume) = if explicit {
                        (
                            self.arg(call, 4)?.integer()? as u32,
                            self.arg(call, 5)?.integer()?,
                        )
                    } else {
                        (fade_ms, -1)
                    };
                    self.channels.crossfade_music(
                        source,
                        mode != 0,
                        crate::sound::Crossfade {
                            duration: fade_ms,
                            overlap,
                            volume,
                        },
                        &self.audio_stop,
                    )
                } else {
                    self.channels
                        .play_music(source, mode != 0, fade_ms, &self.audio_stop)
                }
            }
            651 => {
                self.channels.stop_music();
                0
            }
            656 => self
                .channels
                .fade_out_music(self.arg(call, 0)?.integer()? as u32),
            658 | 659 => self.channels.music_status(call.function == 659),
            660 => self.channels.music_handle(),
            661 => self.channels.resize(self.arg(call, 0)?.integer()?),
            662 => {
                let id = self.arg(call, 0)?.integer()?;
                let volume = self.arg(call, 1)?.integer()?;
                let gain = self.arg(call, 2)?.integer()?;
                self.channels.volume(id, volume, gain)
            }
            663 => self
                .channels
                .set_pan(self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?),
            664 => {
                let id = self.arg(call, 0)?.integer()?;
                let kind = self.arg(call, 1)?.integer()? as u8;
                self.channels.set_kind(id, kind)
            }
            665 | 666 => {
                let id = self.arg(call, 0)?.integer()?;
                let name = self.arg(call, 1)?;
                if !self.channels.valid(id) || name == Word::Integer(0) {
                    return Ok(Some(Word::Integer(1)));
                }
                let name = self.game.vm().text(name.address()?)?;
                let name = encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(&name)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid Lucifen audio resource name",
                        )
                    })?;
                let bytes = match self.game.assets_mut().read(&name, 64 * 1024 * 1024) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        return Ok(Some(Word::Integer(2)));
                    }
                    Err(error) => return Err(error),
                };
                self.channels.load(id, &bytes, &self.audio_stop)
            }
            667 => {
                let id = self.arg(call, 0)?.integer()?;
                self.channels.unload(id);
                0
            }
            668 => {
                let id = self.arg(call, 0)?.integer()?;
                let repeat = self.arg(call, 1)?.integer()? != 0;
                self.channels.start(id, repeat)
            }
            669 => self.channels.stop(self.arg(call, 0)?.integer()?),
            671 => self.fade_in_sound(call)?,
            672 => self.channels.fade_channel_out(
                self.arg(call, 1)?.integer()?,
                self.arg(call, 0)?.integer()? as u32,
            ),
            673 => i32::from(self.channels.playing(self.arg(call, 0)?.integer()?)),
            674 => i32::from(self.channels.finished(self.arg(call, 0)?.integer()?)),
            675 => self.channels.handle(self.arg(call, 0)?.integer()?),
            676 => self.prepare_voice(call)?,
            677 => {
                let channel = self.arg(call, 0)?.integer()?;
                let value = self.arg(call, 1)?;
                let rate = if let Word::Address(address) = value {
                    let bytes = self.game.vm().text(address)?;
                    std::str::from_utf8(&bytes)
                        .ok()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(1.0)
                } else {
                    f64::from(self.application_options.get(3, 14).unwrap_or(50)) / 50.0
                };
                self.channels.speed(channel, rate)
            }

            679 | 680 => self.open_image_button(call)?,
            689 | 690 => self.open_scroll_control(call)?,
            691 => {
                let font = self.arg(call, 5)?.integer()?;
                if font == 0 {
                    0
                } else {
                    let position = [self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?];
                    let layout = [
                        self.arg(call, 2)?.integer()?,
                        self.arg(call, 3)?.integer()?,
                        self.arg(call, 4)?.integer()?,
                    ];
                    let priority = self.arg(call, 6)?.integer()? as u32;
                    self.menu.create_text_list(
                        &mut self.graphics,
                        position,
                        layout,
                        font,
                        priority,
                    )?
                }
            }
            692 => {
                let rect = [
                    self.arg(call, 0)?.integer()?,
                    self.arg(call, 1)?.integer()?,
                    self.arg(call, 2)?.integer()?,
                    self.arg(call, 3)?.integer()?,
                ];
                let x = self.arg(call, 4)?.integer()?;
                let (center, mouse, can_activate) = if x == -1 {
                    ([-1, -1], false, false)
                } else {
                    (
                        [x, self.arg(call, 5)?.integer()?],
                        self.arg(call, 6)?.integer()? != 0,
                        self.arg(call, 7)?.integer()? != 0,
                    )
                };
                self.menu
                    .create_hotspot(&mut self.graphics, rect, center, mouse, can_activate)?
            }
            694 => {
                let target = self.arg(call, 0)?.integer()?;
                if self.graphics.surface(target).is_ok() {
                    self.input.target = Some(target);
                    0
                } else {
                    1
                }
            }
            695 => {
                self.input.cursor_settings = (
                    self.arg(call, 0)?.integer()? != 0,
                    self.arg(call, 1)?.integer()?,
                    self.arg(call, 2)?.integer()?,
                    self.arg(call, 3)?.integer()?,
                );
                0
            }
            696 => {
                let kind = self.arg(call, 0)?.integer()?;
                if (0..=6).contains(&kind) {
                    self.input.cursor_kind = Some(kind);
                    1
                } else {
                    0
                }
            }
            697 => {
                self.menu.reset_input();
                0
            }
            698 => self.menu.update(
                &mut self.input,
                &mut self.graphics,
                &mut self.dialogue.choices_read,
            )?,
            699 => self.menu.take_command(),
            703 => {
                self.menu.free_all(&mut self.graphics);
                0
            }
            704 => self.menu.select_overlay(self.arg(call, 0)?.integer()? != 0),
            705 => {
                self.menu.select_group(self.arg(call, 0)?);
                0
            }
            706 => {
                let group = self.arg(call, 0)?;
                let keep_group = self.arg(call, 1)?.integer()? != 0;
                self.menu.free_group(group, keep_group, &mut self.graphics);
                0
            }
            707 => {
                let pointer = self.arg(call, 0)?.address()?;
                let id = self.game.vm().read(pointer)?.integer()?;
                if self.menu.free(id, &mut self.graphics) {
                    self.game.vm_mut().write(pointer, Word::Integer(0))?;
                    0
                } else {
                    1
                }
            }
            708 => self
                .menu
                .attach_input(self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?),
            709 => {
                let handle = self.arg(call, 0)?.integer()?;
                self.menu.detach_input(handle);
                0
            }
            710 => {
                self.menu.clear_routing();
                0
            }
            713 => self.menu.push_routing(),
            714 => self.menu.pop_routing(),
            715 => {
                let id = self.arg(call, 0)?.integer()?;
                self.menu.set_command(id, self.arg(call, 1)?.integer()?);
                i32::from(id != 0)
            }
            716 => {
                let id = self.arg(call, 0)?.integer()?;
                if id == 0 {
                    0
                } else {
                    let text = match self.arg(call, 1)? {
                        Word::Integer(0) => Vec::new(),
                        word => self.game.vm().text(word.address()?)?,
                    };
                    self.menu.set_tooltip(id, text);
                    1
                }
            }
            719 => {
                let id = self.arg(call, 0)?.integer()?;
                self.menu
                    .set_enabled(id, self.arg(call, 1)?.integer()? != 0)
            }
            720 => i32::from(self.menu.enabled(self.arg(call, 0)?.integer()?)),
            721 => {
                let id = self.arg(call, 0)?.integer()?;
                let visible = self.arg(call, 1)?.integer()? != 0;
                if !self.menu.show_text_list(id, visible, &mut self.graphics) {
                    self.graphics.show_texture(id, visible);
                }
                i32::from(id != 0)
            }
            722 => {
                let id = self.arg(call, 0)?.integer()?;
                i32::from(
                    self.menu
                        .text_lists
                        .get(&id)
                        .map_or_else(|| self.graphics.texture_visible(id), |list| list.visible),
                )
            }
            723 => {
                let id = self.arg(call, 0)?.integer()?;
                let x = self.arg(call, 1)?.integer()?;
                let y = self.arg(call, 2)?.integer()?;
                if !self.menu.move_hotspot(id, [x, y])
                    && !self.menu.move_text_list(id, [x, y], &mut self.graphics)
                {
                    self.graphics.texture_move(id, x, y, false);
                }
                0
            }
            725 | 726 => {
                let id = self.arg(call, 0)?.integer()?;
                self.graphics
                    .texture_bounds(id)
                    .map_or(0, |[x, y, right, bottom]| {
                        if call.function == 725 {
                            right - x
                        } else {
                            bottom - y
                        }
                    })
            }
            727 | 728 => self
                .menu
                .image_handle(self.arg(call, 0)?.integer()?, call.function == 728),
            729 => {
                let handle = self.arg(call, 0)?.integer()?;
                if self.menu.scroll_font(handle).is_some() && !self.menu.is_message_window(handle) {
                    self.append_scroll_text(call)?
                } else if let Word::Integer(value) = self.arg(call, 1)? {
                    self.menu.set_control_value(handle, value)
                } else {
                    -1
                }
            }
            730 => self.menu.selection_state(self.arg(call, 0)?.integer()?),
            731 => {
                let id = self.arg(call, 0)?.integer()?;
                self.menu
                    .rectangular_hit(id, self.arg(call, 1)?.integer()? != 0)
            }
            746 => {
                let id = self.arg(call, 0)?.integer()?;
                i32::from(
                    self.menu.clear_text_rows(id, &mut self.graphics)
                        || self.menu.reset_scroll_text(id, &mut self.graphics),
                )
            }
            747 => self.scroll_caret(call)?,
            734 => self
                .menu
                .radio_selection(self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?),
            741 => {
                let id = self.arg(call, 0)?.integer()?;
                let center = [self.arg(call, 1)?.integer()?, self.arg(call, 2)?.integer()?];
                i32::from(self.menu.set_hotspot_center(id, center))
            }
            742 => {
                let id = self.arg(call, 0)?.integer()?;
                let padding = [
                    self.arg(call, 1)?.integer()?,
                    self.arg(call, 2)?.integer()?,
                    self.arg(call, 3)?.integer()?,
                    self.arg(call, 4)?.integer()?,
                ];
                let inset = [self.arg(call, 5)?.integer()?, self.arg(call, 6)?.integer()?];
                if let Some(list) = self.menu.text_lists.get_mut(&id) {
                    list.padding = padding;
                    list.text_inset = inset;
                    1
                } else if self.menu.scroll_font(id).is_some() {
                    i32::from(self.menu.set_scroll_text_padding(
                        id,
                        padding,
                        [
                            inset[0],
                            inset[1],
                            self.arg(call, 7)?.integer()?,
                            self.arg(call, 8)?.integer()?,
                        ],
                        &mut self.graphics,
                    ))
                } else {
                    0
                }
            }
            744 => {
                let id = self.arg(call, 0)?.integer()?;
                if self.menu.scroll_font(id).is_none() && !self.menu.text_lists.contains_key(&id) {
                    return Ok(Some(Word::Integer(-1)));
                }
                let mode = self.arg(call, 1)?.integer()? as u8 != 0;
                let [red, green, blue] = packed_rgb(self.arg(call, 2)?.integer()? as u32);
                let color = [red, green, blue, self.arg(call, 3)?.integer()? as u8];
                self.menu
                    .set_message_appearance(id, id, mode, color, false, &mut self.graphics)?;
                if let Some((companion, _)) = self.menu.message_companion(id) {
                    self.menu.set_message_appearance(
                        companion,
                        0,
                        mode,
                        color,
                        false,
                        &mut self.graphics,
                    )?;
                }
                0
            }
            753 => {
                let id = self.arg(call, 0)?.integer()?;
                if self
                    .menu
                    .scroll_font(id)
                    .is_none_or(|font| !self.fonts.contains_key(&font))
                {
                    0
                } else {
                    let flow = [
                        self.arg(call, 1)?.integer()?,
                        self.arg(call, 2)?.integer()?,
                        self.arg(call, 3)?.integer()?,
                        self.arg(call, 4)?.integer()?,
                        self.arg(call, 5)?.integer()?,
                        self.arg(call, 6)?.integer()?,
                    ];
                    i32::from(self.menu.set_scroll_flow(id, flow))
                }
            }
            761 => i32::from(!self.menu.is_message_window(self.arg(call, 0)?.integer()?)),
            762 | 763 => self.show_message_window(call)?,
            764 => {
                let handle = self.arg(call, 0)?.integer()?;
                let mode = self.arg(call, 1)?.integer()? as u8;
                if !self.menu.is_message_window(handle) {
                    -1
                } else if !self.menu.message_active(handle) {
                    1
                } else if mode > 3 {
                    2
                } else {
                    0
                }
            }
            765 => {
                let handle = self.arg(call, 0)?.integer()?;
                let strip = self.arg(call, 1)?.integer()?;
                i32::from(self.menu.set_scroll_cursor(handle, strip))
            }
            766 => self.menu.link_scroll_controls(
                self.arg(call, 0)?.integer()?,
                self.arg(call, 1)?.integer()?,
                self.arg(call, 2)?.integer()? as u8,
            ),
            767 => self
                .menu
                .set_history_toggle(self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?),
            768 => {
                let handle = self.arg(call, 0)?.integer()?;
                if self.menu.message_active(handle) {
                    3
                } else {
                    0
                }
            }
            769 => i32::from(self.menu.message_automatic(self.arg(call, 0)?.integer()?)),
            770 => self.message_voice_name(call)?,
            772 => self.open_choice_skin(call)?,
            777 => self.append_text_list_row(call)?,
            773 => {
                let id = self.arg(call, 0)?.integer()?;
                let mode = self.arg(call, 1)?.integer()? as u8;
                let color = self.arg(call, 2)?.integer()? as u32;
                if let Some(list) = self.menu.text_lists.get_mut(&id) {
                    list.highlight = (mode, color);
                    1
                } else {
                    0
                }
            }
            774 => {
                let id = self.arg(call, 0)?.integer()?;
                let colors = [
                    self.arg(call, 1)?.integer()? as u32,
                    self.arg(call, 2)?.integer()? as u32,
                    self.arg(call, 3)?.integer()? as u32,
                    0x909090,
                ];
                if let Some(list) = self.menu.text_lists.get_mut(&id) {
                    list.text_colors = colors;
                    1
                } else {
                    0
                }
            }
            775 => {
                let id = self.arg(call, 0)?.integer()?;
                let mut gradients = [[0; 2]; 4];
                for (index, pair) in gradients[..3].iter_mut().enumerate() {
                    *pair = [
                        self.arg(call, index * 2 + 1)?.integer()? as u32,
                        self.arg(call, index * 2 + 2)?.integer()? as u32,
                    ];
                }
                gradients[3] = [11579568, 5855577];
                if let Some(list) = self.menu.text_lists.get_mut(&id) {
                    list.gradients = gradients;
                    1
                } else {
                    0
                }
            }
            776 => {
                let id = self.arg(call, 0)?.integer()?;
                let inset = [self.arg(call, 1)?.integer()?, self.arg(call, 2)?.integer()?];
                if let Some(list) = self.menu.text_lists.get_mut(&id) {
                    list.text_inset = inset;
                    1
                } else {
                    0
                }
            }
            778 => {
                let handle = self.arg(call, 0)?.integer()?;
                let index = self.arg(call, 1)?.integer()?;
                let state = self.arg(call, 2)?.integer()? as u8;
                self.menu
                    .set_text_row_state(handle, index, state, &mut self.graphics)?
            }
            779 => self.menu.activate_text_list(self.arg(call, 0)?.integer()?),
            780 => self
                .menu
                .copy_text_list_annotations(self.arg(call, 0)?.integer()?),
            _ => return Ok(None),
        };
        Ok(Some(Word::Integer(result)))
    }

    fn bitmap_region(&self, call: NativeCall, parameter: usize) -> io::Result<Option<[i32; 4]>> {
        let left = self.arg(call, parameter)?.integer()?;
        if left == 0xffff {
            return Ok(None);
        }
        Ok(Some([
            left,
            self.arg(call, parameter + 1)?.integer()?,
            self.arg(call, parameter + 2)?.integer()?,
            self.arg(call, parameter + 3)?.integer()?,
        ]))
    }

    fn blend_bitmap_rule(&mut self, call: NativeCall) -> io::Result<i32> {
        let mut handles = [0; 4];
        for (index, handle) in handles.iter_mut().enumerate() {
            *handle = self.arg(call, index)?.integer()?;
        }
        if handles.contains(&0) {
            return Ok(-1);
        }
        let mut size = [0; 2];
        let mut positions = [[0; 2]; 4];
        let x = self.arg(call, 7)?.integer()?;
        if x != 0xffff {
            positions[0] = [x, self.arg(call, 8)?.integer()?];
            let width = self.arg(call, 9)?.integer()?;
            if width != 0xffff {
                size = [width, self.arg(call, 10)?.integer()?];
                for (index, position) in positions[1..].iter_mut().enumerate() {
                    let arg = 11 + 2 * index;
                    let x = self.arg(call, arg)?.integer()?;
                    if x == 0xffff {
                        break;
                    }
                    *position = [x, self.arg(call, arg + 1)?.integer()?];
                }
            }
        }
        let settings = crate::graphics::BitmapRuleBlend {
            progress: self.arg(call, 4)?.integer()? as u8,
            softness: self.arg(call, 5)?.integer()?,
            mode: self.arg(call, 6)?.integer()? as u8,
            size,
            positions,
        };
        Ok(self.graphics.blend_bitmap_rule(handles, settings))
    }

    fn copy_bitmap(&mut self, call: NativeCall) -> io::Result<i32> {
        use crate::graphics::BitmapTarget;
        let target = self.arg(call, 0)?.integer()?;
        let source = self.arg(call, 1)?.integer()?;
        let resolve = |id| {
            if self.graphics.surface(id).is_ok() {
                BitmapTarget::Surface(id)
            } else {
                BitmapTarget::Texture(id)
            }
        };
        let target = resolve(target);
        let Some(source) = self.graphics.bitmap(resolve(source)).cloned() else {
            return Ok(-1);
        };
        let full = [0, 0, i64::from(source.width), i64::from(source.height)];
        let x = self.arg(call, 2)?.integer()?;
        let (position, rect) = if x == 0xffff {
            ([0, 0], full)
        } else {
            let position = [x, self.arg(call, 3)?.integer()?];
            let left = self.arg(call, 4)?.integer()?;
            if left == 0xffff {
                (position, full)
            } else {
                (
                    position,
                    [
                        i64::from(left),
                        i64::from(self.arg(call, 5)?.integer()?),
                        i64::from(self.arg(call, 6)?.integer()?),
                        i64::from(self.arg(call, 7)?.integer()?),
                    ],
                )
            }
        };
        Ok(
            if self
                .graphics
                .stamp_bitmap(target, &source, rect, position, false)
            {
                0
            } else {
                -1
            },
        )
    }

    fn load_bitmap_into(&mut self, call: NativeCall) -> io::Result<i32> {
        use crate::graphics::BitmapTarget;
        let id = self.arg(call, 0)?.integer()?;
        let target = if self.graphics.surface(id).is_ok() {
            BitmapTarget::Surface(id)
        } else {
            BitmapTarget::Texture(id)
        };
        if self.graphics.bitmap(target).is_none() {
            return Ok(-2);
        }
        let name = self.resource_name(call, 1)?;
        let position = [self.arg(call, 2)?.integer()?, self.arg(call, 3)?.integer()?];
        let bytes = match self.game.assets_mut().read(&name, 128 * 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(2),
            Err(error) => return Err(error),
        };
        let source_id = self.graphics.load(&bytes)?;
        let source = self.graphics.surface(source_id)?.clone();
        self.graphics.free(source_id);
        let region = [0, 0, i64::from(source.width), i64::from(source.height)];
        self.graphics
            .stamp_bitmap(target, &source, region, position, false);
        Ok(0)
    }

    fn open_choice_skin(&mut self, call: NativeCall) -> io::Result<i32> {
        let id = self.arg(call, 0)?.integer()?;
        let Some(color) = self.menu.text_lists.get(&id).map(|list| list.background) else {
            return Ok(4);
        };
        let frames = self.arg(call, 1)?.integer()?;
        if !(1..=4).contains(&frames) {
            return Ok(1);
        }
        let disabled_frame = self.arg(call, 3)?.integer()? != 0;
        let source = match self.load_scroll_skin(call, 4) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(1),
            Err(error) => return Err(error),
        };
        let mode = self.application_options.get(2, 6).unwrap_or(0) != 0;
        let Some(bitmap) = source.render(mode, color)? else {
            return Ok(1);
        };
        Ok(self.menu.set_choice_skin(
            id,
            crate::menu::ChoiceSkin {
                bitmap,
                frames: frames as u8,
                disabled_frame,
                source,
            },
        ))
    }

    fn append_text_list_row(&mut self, call: NativeCall) -> io::Result<i32> {
        let id = self.arg(call, 0)?.integer()?;
        let Some(list) = self.menu.text_lists.get(&id) else {
            return Ok(4);
        };
        let input = self.arg(call, 1)?;
        if input == Word::Integer(0) {
            return Ok(1);
        }
        let [width, height, _] = list.layout;
        let Some(atlas_width) = width.checked_mul(2).filter(|&width| width > 0) else {
            return Ok(1);
        };
        if height <= 0 {
            return Ok(1);
        }
        let colors = list.text_colors;
        let visited = list
            .next_choice_key()
            .is_some_and(|key| self.dialogue.choices_read.contains(&key));
        let gradients = list.gradients;
        let skin = list.skin.clone();
        let background = list.background;
        let (highlight_mode, highlight_color) = list.highlight;
        let [left, top, right, bottom] = list.padding;
        let [inset_x, inset_y] = list.text_inset;
        let left = left.clamp(0, width);
        let top = top.clamp(0, height);
        let right = width.saturating_sub(right).clamp(left, width);
        let bottom = height.saturating_sub(bottom).clamp(top, height);
        let text_x = left.saturating_add(inset_x).clamp(0, width);
        let text_y = top.saturating_add(inset_y).clamp(0, height);
        let Some(mut font) = self.fonts.get(&list.font).cloned() else {
            return Ok(1);
        };
        let text = self.game.vm().text(input.address()?)?;
        let text = self.decode_text(&text).into_owned();
        let style = self.arg(call, 3)?.integer()?;
        let disabled = style != 0 && (call.function != 777 || style != -1);
        let command = if call.function == 777 && style != -1 {
            let command = self.arg(call, 5)?.integer()?;
            (command >= 0).then_some(command)
        } else {
            None
        };
        if self.text.is_none() {
            self.text = Some(Text::open(self.text_encoding == encoding_rs::GBK)?);
        }
        let atlas = self.graphics.create(atlas_width as u32, height as u32)?;
        let result = (|| {
            let engine = self.text.as_ref().unwrap();
            let layout = engine.shape(&font, text, (right - text_x).max(1) as u32)?;
            self.graphics.fill(atlas, None, background);
            for (index, color_index) in [usize::from(visited), 2].into_iter().enumerate() {
                let x = index as i32 * width;
                if let Some(skin) = &skin {
                    let frame = if disabled && skin.disabled_frame {
                        skin.frames - 1
                    } else {
                        (index as u8).min(skin.frames - 1)
                    };
                    let left = i64::from(frame) * i64::from(width);
                    self.graphics.stamp_bitmap(
                        crate::graphics::BitmapTarget::Surface(atlas),
                        &skin.bitmap,
                        [left, 0, left + i64::from(width), i64::from(height)],
                        [x, 0],
                        false,
                    );
                }
                if index == 1 && !disabled && highlight_mode != 0 {
                    let [blue, green, red, alpha] = highlight_color.to_le_bytes();
                    self.graphics.fill(
                        atlas,
                        Some([x + left, top, x + right, bottom]),
                        [
                            red,
                            green,
                            blue,
                            if highlight_mode == 1 {
                                background[3]
                            } else {
                                alpha
                            },
                        ],
                    );
                }
                let color_index = if disabled { 3 } else { color_index };
                font.foreground = packed_rgb(colors[color_index]);
                if font.gradient.is_some() {
                    font.gradient = Some(gradients[color_index].map(packed_rgb));
                }
                let raster = engine.rasterize(Arc::clone(&layout), [x + text_x, text_y], [0, 0])?;
                self.graphics.draw_glyphs(atlas, &font, &raster)?;
            }
            self.menu
                .append_text_row(id, atlas, disabled, command, &mut self.graphics)
        })();
        if result.is_err() {
            self.graphics.free(atlas);
        }
        result
    }

    fn append_scroll_text(&mut self, call: NativeCall) -> io::Result<i32> {
        let handle = self.arg(call, 0)?.integer()?;
        let text = self.arg(call, 1)?;
        if text == Word::Integer(0) {
            return Ok(0);
        }
        let bytes = self.game.vm().text(text.address()?)?;
        let text = self.decode_text(&bytes).into_owned();
        self.draw_scroll_text(handle, &text, false, None)
    }

    fn show_message_window(&mut self, call: NativeCall) -> io::Result<i32> {
        let handle = self.arg(call, 0)?.integer()?;
        if !self.menu.is_message_window(handle) {
            return Ok(-1);
        }
        let Some(packet) = &self.dialogue.packet else {
            return Ok(1);
        };
        if call.function == 762 {
            let advance_read = self.arg(call, 1)?.integer()? != 0
                && packet.previously_seen
                && self.application_options.get(1, 4).unwrap_or(0) != 0
                && (self.input.action(4, false) != 0
                    || self.input.action(19, false) != 0
                    || self.input.mouse(0, true));
            self.menu.set_message_automatic(handle, advance_read);
        }
        if let Some(address) = packet.address {
            self.dialogue.seen.insert(address);
        }
        let bytes = packet.message_text(self.text_encoding)?;
        let text = self.decode_text(&bytes).into_owned();
        if let Some((companion, alignment)) = self.menu.message_companion(handle) {
            let name = packet
                .speaker_part(1, self.text_encoding)
                .unwrap_or_default();
            let name = self.decode_text(name).into_owned();
            self.draw_scroll_text(companion, &name, true, Some(alignment))?;
            self.graphics.show_texture(companion, !name.is_empty());
        }
        self.draw_scroll_text(handle, &text, true, None)
    }

    fn fade_in_sound(&mut self, call: NativeCall) -> io::Result<i32> {
        let duration = self.arg(call, 0)?.integer()? as u32;
        let id = self.arg(call, 2)?.integer()?;
        let repeat = self.arg(call, 3)?.integer()? != 0;
        if self.arg(call, 1)? != Word::Integer(0) {
            if !self.channels.valid(id) {
                return Ok(2);
            }
            let name = self.resource_name(call, 1)?;
            let bytes = match self.game.assets_mut().read(&name, 64 * 1024 * 1024) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(2),
                Err(error) => return Err(error),
            };
            if self.channels.load(id, &bytes, &self.audio_stop) != 0 {
                return Ok(2);
            }
        }
        Ok(self.channels.start_fade(id, repeat, duration))
    }

    fn message_voice_name(&mut self, call: NativeCall) -> io::Result<i32> {
        if !self.menu.message_active(self.arg(call, 0)?.integer()?) {
            return Ok(0);
        }
        let Word::Address(destination) = self.arg(call, 1)? else {
            return Ok(0);
        };
        let voice = self
            .dialogue
            .packet
            .as_ref()
            .and_then(|packet| packet.voice_name(self.text_encoding))
            .unwrap_or_default();
        self.game.vm_mut().write_text(destination, &voice)?;
        Ok(i32::from(!voice.is_empty()))
    }

    fn draw_scroll_text(
        &mut self,
        handle: i32,
        text: &str,
        replace: bool,
        alignment: Option<u8>,
    ) -> io::Result<i32> {
        let Some((bitmap, font, settings, content)) =
            self.menu
                .update_scroll_text(handle, text, replace, &mut self.graphics)
        else {
            return Ok(-1);
        };
        if content.is_empty() {
            return Ok(0);
        }
        let Some(mut font) = self.fonts.get(&font).cloned() else {
            return Ok(-1);
        };
        if self.text.is_none() {
            self.text = Some(Text::open(self.text_encoding == encoding_rs::GBK)?);
        }
        font.line_spacing = settings.flow[2];
        let surface = self.graphics.surface(bitmap)?;
        let [width, height] = [surface.width as i32, surface.height as i32];
        let [left, top, right, bottom] = settings.padding;
        let left = left.clamp(0, width - 1);
        let top = top.clamp(0, height - 1);
        let right = width.saturating_sub(right).clamp(left + 1, width);
        let bottom = height.saturating_sub(bottom).clamp(top + 1, height);
        let width = (right - left) as u32;
        let engine = self.text.as_ref().unwrap();
        let layout = engine.shape_with_wrap(
            &font,
            content.to_owned(),
            width,
            roukaku_text::WrapPolicy::WordOrGlyph,
        )?;
        let flow = settings.flow[1];
        let ([x, y], justify) = alignment.map_or(([left, top], 0), |mode| {
            align_scroll_text(
                &layout,
                [left, top, right, bottom],
                settings.overhang,
                mode,
                flow,
            )
        });
        let spacing = [flow, justify];
        let caret = layout.lines.last().map_or([x, y], |line| {
            [
                x.saturating_add(spaced_line_width(&layout, line, spacing).ceil() as i32),
                y.saturating_add(line.top as i32),
            ]
        });
        let raster = engine.rasterize(layout, [x, y], spacing)?;
        self.graphics.draw_glyphs(bitmap, &font, &raster)?;
        self.menu
            .set_scroll_caret(handle, caret, i32::from(font.size));
        Ok(0)
    }

    fn scroll_caret(&mut self, call: NativeCall) -> io::Result<i32> {
        let handle = self.arg(call, 0)?.integer()?;
        let position = if self.menu.text_lists.contains_key(&handle) {
            self.menu
                .text_row_caret(handle, self.arg(call, 3)?.integer()?, &self.graphics)
        } else {
            self.menu
                .scroll_caret(handle, self.arg(call, 4)?.integer()?)
        };
        let Some(position) = position else {
            return Ok(0);
        };
        for (index, value) in position.into_iter().enumerate() {
            if let Word::Address(address) = self.arg(call, index + 1)? {
                self.game.vm_mut().write(address, Word::Integer(value))?;
            }
        }
        Ok(1)
    }

    fn open_scroll_control(&mut self, call: NativeCall) -> io::Result<i32> {
        let position = [self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?];
        let width = self.arg(call, 2)?.integer()?;
        let height = self.arg(call, 3)?.integer()?;
        let font = self.arg(call, 4)?.integer()?;
        if !self.fonts.contains_key(&font) {
            return Ok(0);
        }
        let (skin_source, priority) = if self.arg(call, 5)?.integer()? == -1 {
            (crate::menu::ScrollSkin::default(), 0)
        } else {
            (
                self.load_scroll_skin(call, 6)?,
                self.arg(call, 9)?.integer()? as u32,
            )
        };
        let skin = skin_source.render(skin_source.primary_mask.is_some(), [0, 0, 0, 255])?;
        let size = if let Some(skin) = &skin {
            [skin.width, skin.height]
        } else if width > 0 && height > 0 {
            [width as u32, height as u32]
        } else {
            return Ok(0);
        };
        self.menu.create_scroll_control(
            &mut self.graphics,
            crate::menu::ScrollSetup {
                position,
                size,
                font,
                priority,
                is_backlog: call.function == 690,
                skin,
                skin_source,
            },
        )
    }

    fn load_scroll_skin(
        &mut self,
        call: NativeCall,
        first: usize,
    ) -> io::Result<crate::menu::ScrollSkin> {
        let mut load = |index| {
            if self.arg(call, index)? == Word::Integer(0) {
                return Ok(None);
            }
            let name = self.resource_name(call, index)?;
            let bytes = self.game.assets_mut().read(&name, 64 * 1024 * 1024)?;
            let id = self.graphics.load(&bytes)?;
            let image = self.graphics.surface(id)?.clone();
            self.graphics.free(id);
            Ok::<_, io::Error>(Some(image))
        };
        Ok(crate::menu::ScrollSkin {
            base: load(first)?,
            primary_mask: load(first + 1)?,
            secondary_mask: load(first + 2)?,
        })
    }

    fn open_image_button(&mut self, call: NativeCall) -> io::Result<i32> {
        let from_file = call.function == 679;
        if self.arg(call, 2)? == Word::Integer(0) {
            return Ok(if from_file { 0 } else { -1 });
        }
        let position = [self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?];
        let mode = self.arg(call, 3)?.integer()?;
        let (disabled_frame, press_frame, blend, priority) = if mode == 0xffff {
            (false, true, 2, 0)
        } else {
            (
                mode != 0,
                self.arg(call, 4)?.integer()? != 0,
                self.arg(call, 5)?.integer()? as u8,
                self.arg(call, if from_file { 8 } else { 6 })?.integer()? as u32,
            )
        };
        let atlas = if from_file {
            let name = self.resource_name(call, 2)?;
            let key = if mode == 0xffff {
                [-1; 2]
            } else {
                [self.arg(call, 6)?.integer()?, self.arg(call, 7)?.integer()?]
            };
            let bytes = match self.game.assets_mut().read(&name, 64 * 1024 * 1024) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
                Err(error) => return Err(error),
            };
            let atlas = self.graphics.load(&bytes)?;
            self.graphics.button_color_key(atlas, key);
            atlas
        } else {
            self.arg(call, 2)?.integer()?
        };
        let result = self.menu.create_direct_button(
            crate::menu::DirectButton {
                atlas,
                owns_atlas: from_file,
                position,
                press_frame,
                disabled_frame,
                blend,
                priority,
            },
            &mut self.graphics,
        );
        if from_file && !matches!(result, Ok(id) if id > 0) {
            self.graphics.free(atlas);
        }
        result
    }

    fn open_strip(&mut self, call: NativeCall) -> io::Result<i32> {
        use crate::{graphics::BitmapTarget, strip::Strip};
        let name = self.resource_name(call, 0)?;
        let frames = self.arg(call, 1)?.integer()? as u32;
        let frame_ms = self.arg(call, 2)?.integer()? as u32;
        let sprite = self.arg(call, 3)?.integer()?;
        let surface = self.arg(call, 4)?.integer()?;
        let destination = if sprite != 0 {
            BitmapTarget::Texture(sprite)
        } else {
            BitmapTarget::Surface(surface)
        };
        if frame_ms == 0 || frames == 0 || self.graphics.bitmap(destination).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Lucifen animation strip target or timing",
            ));
        }
        let x = self.arg(call, 5)?.integer()?;
        let y = self.arg(call, 6)?.integer()?;
        let sequence = self.arg(call, 7)?.integer()? as u8;
        let on_stop = self.arg(call, 8)?.integer()? as u8;
        let transparent_flag = self.arg(call, 9)?.integer()?;
        let transparent = transparent_flag != 0 && transparent_flag != 0xffff;
        let color_key = if transparent {
            let key = self.arg(call, 11)?.integer()?;
            (key != -1).then_some(packed_rgb(key as u32))
        } else {
            None
        };
        let bytes = match self.game.assets_mut().read(&name, 64 * 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let image = self.graphics.load_image(&bytes, color_key)?;
        let bitmap = self.graphics.surface(image)?.clone();
        self.graphics.free(image);
        if bitmap.width < frames {
            return Ok(0);
        }
        let position = if x == 0xffff {
            [i32::from(bitmap.origin.0), i32::from(bitmap.origin.1)]
        } else {
            [x, y]
        };
        let clip = self.graphics.next_surface()?;
        self.strips.insert(
            clip,
            Strip {
                bitmap,
                destination,
                position,
                frames,
                frame_ms,
                sequence,
                on_stop,
                transparent,
                redraw: Vec::new(),
            },
        );
        Ok(clip)
    }

    fn position_sprite(&mut self, call: NativeCall, target: i32) -> io::Result<i32> {
        let Some(geometry) = self.graphics.sprite_geometry(target) else {
            return Ok(1);
        };
        let spec = self.resource_name(call, 2)?;
        let placement = crate::effects::SpritePlacement::parse(&spec, geometry)?;
        let [x, y] = placement.position;
        self.graphics.texture_move(target, x, y, false);
        for (index, value) in [x, y, placement.opacity, placement.plane]
            .into_iter()
            .enumerate()
        {
            if let Word::Address(output) = self.arg(call, index + 3)? {
                self.game.vm_mut().write(output, Word::Integer(value))?;
            }
        }
        Ok(0)
    }

    fn move_sprite(&mut self, call: NativeCall) -> io::Result<i32> {
        use crate::animation::{Motion, MotionMode};
        let texture = self.arg(call, 0)?.integer()?;
        let interval = self.arg(call, 1)?.integer()? as u32;
        let offset = usize::from(call.function == 441);
        let Some(path) = self.motion_path(call, offset)? else {
            return Ok(0);
        };
        let delay = self.arg(call, 3 + offset)?.integer()? as u32;
        let hide_after = self.arg(call, 4 + offset)?.integer()? != 0;
        let mode = self.arg(call, 5 + offset)?.integer()?;
        let (mode, curve, ease, repeat) = if mode == 0xffff {
            (MotionMode::Position, false, 0.0, false)
        } else {
            let mode = match mode {
                0 => MotionMode::Opacity,
                1 => MotionMode::Position,
                2 => MotionMode::SourceOffset,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!("unsupported Lucifen sprite path mode {mode}"),
                    ));
                }
            };
            let ease = match self.arg(call, 7 + offset)? {
                Word::Integer(0) => 0.0,
                word => {
                    let text = self.game.vm().text(word.address()?)?;
                    std::str::from_utf8(&text)
                        .ok()
                        .and_then(|text| text.trim().parse::<f64>().ok())
                        .filter(|ease| ease.is_finite())
                        .unwrap_or(0.0)
                }
            };
            (
                mode,
                self.arg(call, 6 + offset)?.integer()? != 0,
                ease,
                self.arg(call, 8 + offset)?.integer()? != 0,
            )
        };
        self.animations.start_motion(
            Motion {
                texture,
                interval,
                delay,
                hide_after,
                path,
                mode,
                curve,
                ease,
                repeat,
            },
            self.timers.now(),
            &mut self.graphics,
        )
    }

    fn motion_path(
        &self,
        call: NativeCall,
        offset: usize,
    ) -> io::Result<Option<Vec<crate::animation::MotionPoint>>> {
        use crate::animation::MotionPoint;
        let Word::Address(address) = self.arg(call, 2 + offset)? else {
            return Ok(None);
        };
        if offset == 0 {
            let specification = self.game.vm().text(address)?;
            let data = std::str::from_utf8(&specification).map_err(io::Error::other)?;
            let numbers = data
                .split([' ', '\t', '(', ')', ','])
                .filter(|value| !value.is_empty())
                .map(motion_number)
                .collect::<io::Result<Vec<_>>>()?;
            if numbers.is_empty() || numbers.len() % 3 != 0 || numbers.len() / 3 > 65_536 {
                return Ok(None);
            }
            return Ok(Some(
                numbers
                    .chunks_exact(3)
                    .map(|point| MotionPoint {
                        position: [point[0], point[1]],
                        opacity: point[2] as u8,
                    })
                    .collect(),
            ));
        }
        let count = self.arg(call, 2)?.integer()?;
        if !(1..=65_536).contains(&count) {
            return Ok(None);
        }
        let mut points = Vec::with_capacity(count as usize);
        for index in 0..i64::from(count) {
            let value = |field: i64| {
                let address = crate::vm::displace(address, index * 12 + field * 4)?;
                self.game.vm().read(address)?.integer()
            };
            points.push(MotionPoint {
                position: [value(0)?, value(1)?],
                opacity: value(2)? as u8,
            });
        }
        Ok(Some(points))
    }

    fn start_particles(
        &mut self,
        call: NativeCall,
        target: i32,
        kind: crate::effects::ParticleKind,
    ) -> io::Result<i32> {
        let count = self.arg(call, 2)?.integer()?;
        if count <= 0 {
            return Ok(0);
        }
        let settings = crate::effects::ParticleSettings {
            kind,
            count: count as usize,
            priority: self.arg(call, 3)?.integer()?,
            priority_step: self.arg(call, 4)?.integer()?,
            prefill: self.arg(call, 5)?.integer()? != 0,
            visible: [
                self.arg(call, 6)?.integer()? != 0,
                self.arg(call, 7)?.integer()? != 0,
            ],
        };
        let mut samples = vec![0; kind.atlases()];
        let result = (|| {
            for (index, sample) in samples.iter_mut().enumerate() {
                let name = kind.atlas_name(index);
                let bytes = self.game.assets_mut().read(&name, 64 * 1024 * 1024)?;
                *sample = self.graphics.load(&bytes)?;
            }
            self.effects.start_particles(
                target,
                &samples,
                settings,
                self.timers.now(),
                &mut self.graphics,
                &mut self.random,
            )
        })();
        for sample in samples {
            self.graphics.free(sample);
        }
        result
    }

    fn start_background_transform(&mut self, call: NativeCall, target: i32) -> io::Result<i32> {
        use crate::effects::{BackgroundTransformPath, BackgroundTransformSettings};

        let (path, args) = if self.arg(call, 2)? == Word::Integer(0) {
            let center = [self.arg(call, 3)?.integer()?, self.arg(call, 4)?.integer()?];
            let scale = |index| {
                if self.arg(call, index)? == Word::Integer(0) {
                    Ok(1.0)
                } else {
                    self.effect_number(call, index)
                }
            };
            let turns = |index| {
                self.effect_number(call, index)
                    .map(|value| (value * -512.0).trunc())
            };
            (
                BackgroundTransformPath::Centered {
                    center,
                    scale: [scale(5)?, scale(7)?],
                    turns: [turns(6)?, turns(8)?],
                },
                9,
            )
        } else {
            let spec = self.resource_name(call, 2)?;
            let smooth = self.arg(call, 3)?.integer()? != 0;
            (BackgroundTransformPath::parse(&spec, smooth)?, 4)
        };
        let bitmap = if self.arg(call, args + 3)? == Word::Integer(0) {
            None
        } else {
            let name = self.resource_name(call, args + 3)?;
            let bytes = self.game.assets_mut().read(&name, 64 * 1024 * 1024)?;
            let handle = self.graphics.load(&bytes)?;
            let bitmap = self.graphics.surface(handle)?.clone();
            self.graphics.free(handle);
            Some(bitmap)
        };
        let settings = BackgroundTransformSettings {
            path,
            bitmap,
            duration: self.arg(call, args)?.integer()? as u32,
            opacity: self.arg(call, args + 1)?.integer()? as u8,
            ease: self.effect_number(call, args + 2)?,
            flip: [
                self.arg(call, args + 4)?.integer()? != 0,
                self.arg(call, args + 5)?.integer()? != 0,
            ],
            in_place: self.arg(call, args + 6)?.integer()? != 0,
            sampling: self.arg(call, args + 7)?.integer()? as u8,
            remove_on_finish: self.arg(call, args + 9)?.integer()? != 0,
        };
        self.effects.start_background_transform(
            target,
            settings,
            self.timers.now(),
            &mut self.graphics,
        )
    }

    fn effect_number(&self, call: NativeCall, parameter: usize) -> io::Result<f64> {
        if self.arg(call, parameter)? == Word::Integer(0) {
            return Ok(0.0);
        }
        Ok(self
            .resource_name(call, parameter)?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .unwrap_or(0.0))
    }

    fn resource_name(&self, call: NativeCall, parameter: usize) -> io::Result<String> {
        let bytes = self.game.vm().text(self.arg(call, parameter)?.address()?)?;
        encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(&bytes)
            .map(|value| value.into_owned())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid CP932 Lucifen resource name",
                )
            })
    }

    fn read_menu_controls(&mut self, call: NativeCall) -> io::Result<i32> {
        let start = self.arg(call, 0)?.integer()?;
        let (start, count, output) = if call.function == 523 {
            (start as usize, 1, 1)
        } else {
            let requested = self.arg(call, 1)?.integer()?;
            let count = if requested == 0 {
                self.menu.controls.len()
            } else {
                requested.max(0) as usize
            };
            (start.max(0) as usize, count, 2)
        };
        let Some(controls) = self.menu.controls.get(start..) else {
            return Ok(0);
        };
        let count = count.min(controls.len());
        if count == 0 {
            return Ok(0);
        }
        let mut outputs = [None; 4];
        for (index, address) in outputs.iter_mut().enumerate() {
            match self.arg(call, output + index)? {
                Word::Integer(0) => {}
                word => *address = Some(word.address()?),
            }
        }
        let vm = self.game.vm_mut();
        for (index, control) in controls[..count].iter().enumerate() {
            let values = [
                control.handle,
                i32::from(control.extent[0]),
                i32::from(control.extent[1]),
                i32::from(control.states),
            ];
            for (address, value) in outputs.into_iter().zip(values) {
                if let Some(address) = address {
                    vm.write(
                        crate::vm::displace(address, (index * 4) as i64)?,
                        Word::Integer(value),
                    )?;
                }
            }
        }
        Ok(1)
    }

    fn prepare_voice(&mut self, call: NativeCall) -> io::Result<i32> {
        let Some(voices) = &self.voices else {
            return Ok(-5);
        };
        let channel = self.arg(call, 0)?.integer()?;
        let name = self.game.vm().text(self.arg(call, 1)?.address()?)?;
        let Some((index, group)) = voices.lookup(&name) else {
            return Ok(-3);
        };
        if channel < 0 {
            return Ok(if channel == -1 {
                index as i32
            } else {
                group as i32
            });
        }
        let Some(voice) = voices.group(group) else {
            return Ok(-3);
        };
        if !voice.enabled || !self.channels.valid(channel) {
            return Ok(-2);
        }
        let gain = voice.gain;
        let (clip, _) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&name);
        let (extension, _) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&voices.extension);
        let path = format!("voice/{clip}.{extension}");
        let bytes = match self.game.assets_mut().read(&path, 64 * 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(-4),
            Err(error) => return Err(error),
        };
        if self.channels.load(channel, &bytes, &self.audio_stop) != 0 {
            return Ok(-4);
        }
        self.channels.volume(channel, -1, gain);
        let count = self.arg(call, 2)?.integer()?;
        if count > 0 {
            for position in 0..count.min(4096) {
                if self.arg(call, position as usize + 3)?.integer()? == index as i32 {
                    return Ok(position);
                }
            }
            Ok(-1)
        } else {
            Ok(index as i32)
        }
    }

    fn format_text(&self, call: NativeCall, format_index: usize) -> io::Result<Vec<u8>> {
        let syntax = || {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported Lucifen text format",
            )
        };
        let address = self.arg(call, format_index)?.address()?;
        let format = self.game.vm().text(address)?;
        let mut input = format.iter().copied().peekable();
        let mut output = Vec::new();
        let mut parameter = format_index + 1;
        while let Some(byte) = input.next() {
            if byte != b'%' {
                output.push(byte);
            } else if input.peek() == Some(&b'%') {
                input.next();
                output.push(b'%');
            } else {
                let mut left = false;
                let mut zero = false;
                let mut plus = false;
                let mut alternate = false;
                while let Some(flag) = input.peek().copied() {
                    match flag {
                        b'-' => left = true,
                        b'0' => zero = true,
                        b'+' => plus = true,
                        b'#' => alternate = true,
                        _ => break,
                    }
                    input.next();
                }
                let mut width = 0usize;
                while let Some(digit) = input.peek().filter(|byte| byte.is_ascii_digit()) {
                    width = width * 10 + usize::from(*digit - b'0');
                    if width > 1024 {
                        return Err(syntax());
                    }
                    input.next();
                }
                if matches!(input.peek(), Some(b'l' | b'h')) {
                    input.next();
                }
                let kind = input.next().ok_or_else(syntax)?;
                let value = self.arg(call, parameter)?;
                parameter += 1;
                let integer = || value.integer();
                let (prefix, bytes) = match kind {
                    b's' => (b"".as_slice(), self.game.vm().text(value.address()?)?),
                    b'c' => (b"".as_slice(), vec![integer()? as u8]),
                    b'd' | b'i' => {
                        let number = integer()?;
                        let sign = if number < 0 {
                            b"-".as_slice()
                        } else if plus {
                            b"+".as_slice()
                        } else {
                            b"".as_slice()
                        };
                        (sign, number.unsigned_abs().to_string().into_bytes())
                    }
                    b'u' => (b"".as_slice(), (integer()? as u32).to_string().into_bytes()),
                    b'x' | b'X' => {
                        let value = integer()? as u32;
                        let digits = if kind == b'x' {
                            format!("{value:x}")
                        } else {
                            format!("{value:X}")
                        };
                        let prefix = match (kind, alternate) {
                            (b'x', true) => b"0x".as_slice(),
                            (b'X', true) => b"0X".as_slice(),
                            _ => b"".as_slice(),
                        };
                        (prefix, digits.into_bytes())
                    }
                    _ => return Err(syntax()),
                };
                let padding = width.saturating_sub(bytes.len() + prefix.len());
                if left {
                    output.extend_from_slice(prefix);
                    output.extend_from_slice(&bytes);
                    output.extend(std::iter::repeat_n(b' ', padding));
                } else if zero && !matches!(kind, b's' | b'c') {
                    output.extend_from_slice(prefix);
                    output.extend(std::iter::repeat_n(b'0', padding));
                    output.extend_from_slice(&bytes);
                } else {
                    output.extend(std::iter::repeat_n(b' ', padding));
                    output.extend_from_slice(prefix);
                    output.extend_from_slice(&bytes);
                }
            }
            if output.len() > 1023 {
                return Err(syntax());
            }
        }
        Ok(output)
    }

    fn rasterize_text(&mut self, call: NativeCall) -> io::Result<i32> {
        let Word::Address(address) = self.arg(call, 0)? else {
            return Ok(0);
        };
        let raw = self.game.vm().text(address)?;
        if raw.is_empty() {
            return Ok(0);
        }
        let font_id = self.arg(call, 1)?.integer()?;
        let font_id = if font_id == 0 {
            self.font
        } else {
            Some(font_id)
        };
        let Some(font) = font_id.and_then(|id| self.fonts.get(&id)) else {
            return Ok(0);
        };
        let text = self.decode_text(&raw).into_owned();
        if self.text.is_none() {
            self.text = Some(Text::open(self.text_encoding == encoding_rs::GBK)?);
        }
        let engine = self.text.as_ref().unwrap();
        let layout = engine.shape(font, text, 16_384)?;
        let width = layout
            .lines
            .iter()
            .map(|line| line.width.ceil() as u32)
            .max()
            .unwrap_or(0);
        let height = layout.height.ceil() as u32;
        let shadow_x = if font.effect.0 != 0 { font.effect.1 } else { 0 };
        let shadow_y = if font.effect.0 != 0 { font.effect.2 } else { 0 };
        let raster = engine.rasterize(
            layout,
            [
                shadow_x.saturating_neg().max(0),
                shadow_y.saturating_neg().max(0),
            ],
            [0, 0],
        )?;
        let bitmap = self.graphics.create(
            width.saturating_add(shadow_x.unsigned_abs()).max(1),
            height.saturating_add(shadow_y.unsigned_abs()).max(1),
        )?;
        if let Err(error) = self.graphics.draw_glyphs(bitmap, font, &raster) {
            self.graphics.free(bitmap);
            return Err(error);
        }
        Ok(bitmap)
    }

    fn measure_text(&mut self, call: NativeCall) -> io::Result<i32> {
        let id = self.arg(call, 0)?.integer()?;
        let Some(font) = self.fonts.get(&id) else {
            return Ok(-1);
        };
        let Word::Address(source) = self.arg(call, 1)? else {
            return Ok(0);
        };
        let bytes = self.game.vm().text(source)?;
        if bytes.is_empty() {
            return Ok(0);
        }
        let value = self.decode_text(&bytes).into_owned();
        if self.text.is_none() {
            self.text = Some(Text::open(self.text_encoding == encoding_rs::GBK)?);
        }
        let engine = self.text.as_ref().unwrap();
        let layout = engine.shape(font, value, 16_384)?;
        let width = layout
            .lines
            .iter()
            .map(|line| line.width.ceil() as i32)
            .max()
            .unwrap_or(0);
        let height = layout.height.ceil() as i32;
        // Bearings are VM-visible and include the raster mask's overhang.
        let raster = engine.rasterize(layout, [0, 0], [0, 0])?;
        let (left, top) = raster.glyphs().fold((0, 0), |(left, top), (_, [x, y])| {
            (left.min(x as i32), top.min(y as i32))
        });
        let sizes = [Word::Integer(width), Word::Integer(height)];
        if call.function == 183 {
            let output = self.arg(call, 2)?.address()?;
            self.game.vm_mut().write(output, sizes[0])?;
            self.game
                .vm_mut()
                .write(crate::vm::displace(output, 4)?, sizes[1])?;
        } else {
            for (index, size) in sizes.into_iter().enumerate() {
                let output = self.arg(call, 2 + index)?.address()?;
                self.game.vm_mut().write(output, size)?;
            }
        }
        for (index, value) in [left, top].into_iter().enumerate() {
            if let Word::Address(output) = self.arg(call, 4 + index)? {
                self.game.vm_mut().write(output, Word::Integer(value))?;
            }
        }
        Ok(1)
    }

    fn draw_text(&mut self, call: NativeCall) -> io::Result<i32> {
        let surface = self.arg(call, 0)?.integer()?;
        let (x, y) = (self.arg(call, 1)?.integer()?, self.arg(call, 2)?.integer()?);
        let raw = if call.function == 185 {
            self.format_text(call, 3)?
        } else {
            self.game.vm().text(self.arg(call, 3)?.address()?)?
        };
        if raw.is_empty() {
            return Ok(1);
        }
        let font = if call.function == 185 {
            self.font
        } else {
            let id = self.arg(call, 4)?.integer()?;
            if id == 0 { self.font } else { Some(id) }
        };
        let Some(font) = font.filter(|id| self.fonts.contains_key(id)) else {
            return Ok(2);
        };
        let value = self.decode_text(&raw).into_owned();
        self.draw_surface_text(surface, x, y, font, value, 0)?;
        Ok(0)
    }

    fn draw_surface_text(
        &mut self,
        surface: i32,
        x: i32,
        y: i32,
        font: i32,
        value: String,
        spacing: i32,
    ) -> io::Result<()> {
        if value.is_empty() {
            return Ok(());
        }
        let width = self.graphics.surface(surface)?.width;
        let font = self.fonts.get(&font).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "unknown Lucifen text font")
        })?;
        if self.text.is_none() {
            self.text = Some(Text::open(self.text_encoding == encoding_rs::GBK)?);
        }
        let engine = self.text.as_ref().unwrap();
        let layout = engine.shape(font, value, width)?;
        let raster = engine.rasterize(layout, [x, y], [spacing, 0])?;
        self.graphics.draw_glyphs(surface, font, &raster)
    }

    fn open_dialogue(&mut self, call: NativeCall) -> io::Result<i32> {
        if !self.dialogue_enabled {
            return Ok(-1);
        }
        let address = self.arg(call, 0)?.address()?;
        let mut packet =
            crate::dialogue::Packet::read(self.game.vm(), address, self.text_encoding)?;
        packet.previously_seen = self.dialogue.seen.contains(&address);
        if self.backlog_enabled {
            self.slot_history.push(&packet, self.backlog_limit)?;
        }
        self.dialogue.packet = Some(packet);
        Ok(0)
    }

    fn set_dialogue_destination(&mut self, call: NativeCall) -> io::Result<i32> {
        let surface = self.arg(call, 0)?.integer()?;
        if self.graphics.surface(surface).is_err() {
            return Ok(1);
        }
        let font = self.arg(call, 4)?.integer()?;
        if !self.fonts.contains_key(&font) {
            return Ok(2);
        }
        self.dialogue.destination = Some(crate::dialogue::Destination {
            surface,
            font,
            x: self.arg(call, 2)?.integer()?,
            y: self.arg(call, 3)?.integer()?,
            spacing: self.arg(call, 7)?.integer()?,
        });
        Ok(0)
    }

    fn dialogue_speaker(&mut self, call: NativeCall, draw: bool) -> io::Result<i32> {
        let part = self.arg(call, if draw { 2 } else { 1 })?.integer()? as usize;
        let name = self
            .dialogue
            .packet
            .as_ref()
            .and_then(|packet| packet.speaker_part(part, self.text_encoding));
        if !draw {
            let Some(name) = name else { return Ok(0) };
            let length = name.len() as i32 + 1;
            if let Word::Address(address) = self.arg(call, 0)? {
                self.game.vm_mut().write_text(address, name)?;
            }
            return Ok(length);
        }
        let surface = self.arg(call, 3)?.integer()?;
        let surface = if surface == 0 {
            self.dialogue.destination.map_or(0, |dest| dest.surface)
        } else {
            surface
        };
        let font = if self.arg(call, 4)?.integer()? as u8 == 255 {
            0
        } else {
            self.arg(call, 5)?.integer()?
        };
        let font = if font == 0 {
            self.dialogue.destination.map_or(0, |dest| dest.font)
        } else {
            font
        };
        if self.graphics.surface(surface).is_err() || !self.fonts.contains_key(&font) {
            return Ok(1);
        }
        let Some(name) = name else { return Ok(2) };
        let value = self.decode_text(name).into_owned();
        self.draw_surface_text(
            surface,
            self.arg(call, 0)?.integer()?,
            self.arg(call, 1)?.integer()?,
            font,
            value,
            0,
        )?;
        Ok(0)
    }

    fn scan_dialogue(&mut self, skip: i32, draw: bool) -> io::Result<i32> {
        if draw && self.dialogue.destination.is_none() {
            return Ok(-1);
        }
        let Some(packet) = &mut self.dialogue.packet else {
            return Ok(-1);
        };
        let status = packet.scan(skip, self.text_encoding)?;
        if draw {
            self.draw_dialogue()?;
        }
        Ok(status)
    }

    fn draw_dialogue(&mut self) -> io::Result<bool> {
        let (Some(destination), Some(packet)) =
            (self.dialogue.destination, &mut self.dialogue.packet)
        else {
            return Ok(false);
        };
        let Some(text) = packet.take_text(self.text_encoding)? else {
            return Ok(false);
        };
        if let Some(address) = packet.address {
            self.dialogue.seen.insert(address);
        }
        self.draw_surface_text(
            destination.surface,
            destination.x,
            destination.y,
            destination.font,
            text,
            destination.spacing,
        )?;
        Ok(true)
    }

    fn dialogue_summary(&mut self, call: NativeCall) -> io::Result<i32> {
        let Some(packet) = &self.dialogue.packet else {
            return Ok(0);
        };
        let text = packet.summary(self.text_encoding)?;
        if let Word::Address(address) = self.arg(call, 0)? {
            self.game.vm_mut().write_text(address, &text)?;
        }
        Ok(text.len() as i32 + 1)
    }

    fn draw_annotation(&mut self, call: NativeCall) -> io::Result<i32> {
        let index = self.arg(call, 0)?.integer()? as usize;
        let Some(label) = self.dialogue.annotation_label(index) else {
            return Ok(2);
        };
        let surface = self.arg(call, 3)?.integer()?;
        let surface = if surface == 0 {
            self.dialogue.destination.map_or(0, |target| target.surface)
        } else {
            surface
        };
        let font = if self.arg(call, 4)?.integer()? as u8 == 255 {
            0
        } else {
            self.arg(call, 5)?.integer()?
        };
        let font = if font == 0 {
            self.dialogue.destination.map_or(0, |target| target.font)
        } else {
            font
        };
        if self.graphics.surface(surface).is_err() || !self.fonts.contains_key(&font) {
            return Ok(1);
        }
        let text = self.decode_text(label).into_owned();
        self.draw_surface_text(
            surface,
            self.arg(call, 1)?.integer()?,
            self.arg(call, 2)?.integer()?,
            font,
            text,
            0,
        )?;
        Ok(0)
    }

    fn copy_annotation(&mut self, call: NativeCall) -> io::Result<i32> {
        let index = self.arg(call, 0)?.integer()? as usize;
        let Some(label) = self.dialogue.annotation_label(index) else {
            return Ok(1);
        };
        let capacity = self.arg(call, 2)?.integer()?;
        if label.len() as i32 >= capacity {
            return Ok(3);
        }
        let output = self.arg(call, 1)?.address()?;
        self.game.vm_mut().write_text(output, label)?;
        Ok(0)
    }

    fn annotation_address(&mut self, call: NativeCall) -> io::Result<Word> {
        let index = self.arg(call, 0)?.integer()? as usize;
        if self.dialogue.annotation_label(index).is_none() {
            return Ok(Word::Integer(0));
        }
        let offset = self.dialogue.annotations[..index]
            .iter()
            .map(Vec::len)
            .sum::<usize>()
            + 2;
        let image = self
            .dialogue
            .annotations
            .iter()
            .flatten()
            .copied()
            .collect();
        let module = self
            .game
            .vm_mut()
            .host_image(&mut self.dialogue.annotation_image, image)?;
        let offset = u32::try_from(offset).map_err(io::Error::other)?;
        Ok(Word::Address(crate::Address::Image { module, offset }))
    }

    fn select_history(&mut self, call: NativeCall) -> io::Result<i32> {
        let count = self.slot_history.entry_count();
        if count <= 0 {
            return Ok(-1);
        }
        let current = self.slot_history.position;
        let index = match call.function {
            479 => current - 1,
            480 => current + 1,
            481 => 0,
            482 => count - 1,
            483 => self.arg(call, 0)?.integer()?.clamp(0, count - 1),
            484 => current
                .saturating_add(self.arg(call, 0)?.integer()?)
                .clamp(0, count - 1),
            _ => unreachable!("Lucifen history navigation operation"),
        };
        let Some(packet) = self.slot_history.select(index)? else {
            return Ok(-1);
        };
        self.dialogue.packet = Some(packet);
        Ok(0)
    }

    fn existing_slot_name(&self, group: i32, slot: i32) -> io::Result<Option<String>> {
        if slot >= 0 {
            return Ok(slot_file_name(group, slot));
        }
        let Some((head, first)) = self.application_options.slot_ring(group) else {
            return Ok(None);
        };
        let length = 10 - first;
        for distance in 1..=length {
            let slot = (head - first - distance).rem_euclid(length) + first;
            let name = slot_file_name(group, slot).unwrap();
            if self.system.save_root.join(&name).try_exists()? {
                return Ok(Some(name));
            }
        }
        Ok(None)
    }

    fn slot_signature(&mut self) -> io::Result<[u8; 4]> {
        if let Some(signature) = self.slot_signature {
            return Ok(signature);
        }
        let signature = crate::app::slot_signature(&self.root)?;
        self.slot_signature = Some(signature);
        Ok(signature)
    }

    fn read_slot(&mut self, call: NativeCall) -> io::Result<i32> {
        let Some(name) =
            self.existing_slot_name(self.arg(call, 0)?.integer()?, self.arg(call, 1)?.integer()?)?
        else {
            return Ok(-1);
        };
        let Some(bytes) = self.read_save_file(&name)? else {
            return Ok(1);
        };
        let signature = self.slot_signature()?;
        let slot = match shinki_emu_lucifen_formats::SaveSlot::decode(&bytes, signature) {
            Ok(slot) => slot,
            Err(_) => return Ok(3),
        };
        if self.restore_slot_state(&slot.state).is_err() {
            return Ok(4);
        }
        self.slot_chapter = slot.chapter;
        self.slot_summary = slot.summary;
        Ok(0)
    }

    fn restore_slot_state(&mut self, state: &[u8]) -> io::Result<()> {
        let (history, checkpoint) = self.game.vm_mut().decode_slot(state)?;
        self.dialogue.clear_message();
        self.menu.bind_text_list_annotations(None);
        self.slot_history = history;
        self.slot_checkpoint = checkpoint;
        Ok(())
    }

    fn describe_slot(&mut self, call: NativeCall) -> io::Result<i32> {
        use shinki_emu_lucifen_formats::SaveSlotPreview;
        use std::{fs::File, io::Read};

        let Word::Address(destination) = self.arg(call, 0)? else {
            return Ok(1);
        };
        let Some(name) =
            self.existing_slot_name(self.arg(call, 1)?.integer()?, self.arg(call, 2)?.integer()?)?
        else {
            return Ok(-1);
        };
        let mut file = match File::open(self.system.save_root.join(name)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(2),
            Err(error) => return Err(error),
        };
        let mut bytes = vec![0; SaveSlotPreview::SIZE];
        if let Err(error) = file.read_exact(&mut bytes) {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                return Ok(2);
            }
            return Err(error);
        }
        let preview = SaveSlotPreview::decode(&bytes)?;
        let chapter = preview
            .chapter
            .split(|&byte| byte == 0)
            .next()
            .unwrap_or_default();
        let separator = match self.arg(call, 3)?.integer()? as u8 {
            0 => " ",
            1 => "\n",
            _ => "\r\n",
        };
        let date = slot_date(file.metadata()?.modified()?);
        let suffix = format!("』{separator}【記録日時 {date}】");
        let (prefix, _, _) = self.text_encoding.encode("『");
        let mut description = prefix.into_owned();
        description.extend_from_slice(chapter);
        description.extend_from_slice(&self.text_encoding.encode(&suffix).0);
        self.game.vm_mut().write_text(destination, &description)?;
        Ok(0)
    }

    fn preview_slot(&mut self, call: NativeCall) -> io::Result<i32> {
        use crate::graphics::BitmapTarget;
        use shinki_emu_lucifen_formats::{SaveSlotPreview, THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH};
        use std::{fs::File, io::Read};

        let group = self.arg(call, 0)?.integer()?;
        let slot = self.arg(call, 1)?.integer()?;
        let open = |slot| {
            File::open(
                self.system
                    .save_root
                    .join(format!("DATA{group:02}{slot:02}.SAV")),
            )
        };
        let mut file = if slot >= 0 {
            if !(0..=99).contains(&group) || slot > 99 {
                return Ok(-1);
            }
            match open(slot) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(-1),
                Err(error) => return Err(error),
            }
        } else if let Some((latest, first)) = self.application_options.slot_ring(group) {
            let mut file = None;
            let count = 10 - first;
            for step in 1..=count {
                let slot = first + (latest - first - step).rem_euclid(count);
                match open(slot) {
                    Ok(candidate) => {
                        file = Some(candidate);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            let Some(file) = file else { return Ok(-1) };
            file
        } else {
            return Ok(-1);
        };
        let mut bytes = vec![0; SaveSlotPreview::SIZE];
        if let Err(error) = file.read_exact(&mut bytes) {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                return Ok(2);
            }
            return Err(error);
        }
        let preview = SaveSlotPreview::decode(&bytes)?;
        let bitmap = self.arg(call, 2)?.integer()?;
        if bitmap != 0 {
            let target = if self.graphics.surface(bitmap).is_ok() {
                BitmapTarget::Surface(bitmap)
            } else {
                BitmapTarget::Texture(bitmap)
            };
            if !self.graphics.write_bgr_bitmap(
                target,
                THUMBNAIL_WIDTH as u32,
                THUMBNAIL_HEIGHT as u32,
                preview.thumbnail_bgr,
            )? {
                return Ok(1);
            }
        }
        if let Word::Address(address) = self.arg(call, 3)? {
            self.game.vm_mut().write_data(address, preview.chapter)?;
        }
        if let Word::Address(address) = self.arg(call, 4)? {
            if self.arg(call, 6)?.integer()? as u8 == 0 {
                self.game.vm_mut().write_data(address, preview.summary)?;
            } else {
                let summary =
                    wrap_slot_summary(preview.summary, self.text_encoding == encoding_rs::GBK);
                self.game.vm_mut().write_text(address, &summary)?;
            }
        }
        if let Word::Address(address) = self.arg(call, 5)? {
            let date = slot_date(file.metadata()?.modified()?);
            let (date, _, _) = self.text_encoding.encode(&date);
            self.game.vm_mut().write_text(address, &date)?;
        }
        Ok(0)
    }

    fn write_slot(&mut self, call: NativeCall) -> io::Result<i32> {
        use shinki_emu_lucifen_formats::{SaveSlot, THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH};
        let group = self.arg(call, 0)?.integer()?;
        let requested = self.arg(call, 1)?.integer()?;
        let ring = (requested < 0)
            .then(|| self.application_options.slot_ring(group))
            .flatten();
        let index = ring.map_or(requested, |(head, first)| head.clamp(first, 9));
        let Some(name) = slot_file_name(group, index) else {
            return Ok(-1);
        };
        let signature = self.slot_signature()?;
        let want_thumbnail = !self.thumbnail_condition(1)?;
        let mut thumbnail = Vec::with_capacity(THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT * 3);
        if want_thumbnail {
            let Some(target) = self.thumbnail_surface else {
                return Ok(1);
            };
            let Some(screen) = self
                .graphics
                .save_thumbnail_source(target, self.thumbnail_condition(0)?)?
            else {
                return Ok(1);
            };
            for y in 0..THUMBNAIL_HEIGHT {
                let row = y * screen.height as usize / THUMBNAIL_HEIGHT * screen.width as usize;
                for x in 0..THUMBNAIL_WIDTH {
                    let offset = (row + x * screen.width as usize / THUMBNAIL_WIDTH) * 4;
                    let pixel = &screen.pixels()[offset..][..4];
                    thumbnail.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                }
            }
        } else {
            thumbnail.resize(THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT * 3, 0);
        }
        let include_history = self.application_options.get(1, 6).unwrap_or(0) != 0;
        let state = self.game.vm_mut().encode_slot(
            &self.slot_history,
            include_history,
            self.slot_checkpoint.as_deref(),
        )?;
        let slot = SaveSlot {
            signature,
            thumbnail_bgr: thumbnail,
            chapter: self.slot_label(0, self.slot_chapter, 40)?,
            summary: self.slot_label(1, self.slot_summary, 139)?,
            state,
        };
        self.write_save_file(&name, &slot.encode()?)?;
        let next = ring.map(|(_, first)| if index == 9 { first } else { index + 1 });
        let mut application_options = self.application_options.clone();
        application_options.record_slot_write(group, index, next);
        self.write_save_file("APP_CFG.DAT", &application_options.encode())?;
        self.application_options = application_options;
        Ok(0)
    }

    fn slot_label<const N: usize>(
        &self,
        label: usize,
        previous: [u8; N],
        limit: usize,
    ) -> io::Result<[u8; N]> {
        let Some(address) = self.slot_labels[label] else {
            return Ok(previous);
        };
        let value = self.game.vm().text(address)?;
        let mut count = value.len().min(limit);
        if count < value.len() {
            while count > 0
                && self
                    .text_encoding
                    .decode_without_bom_handling_and_without_replacement(&value[..count])
                    .is_none()
            {
                count -= 1;
            }
        }
        let mut output = [0; N];
        output[..count].copy_from_slice(&value[..count]);
        Ok(output)
    }

    fn thumbnail_condition(&self, index: usize) -> io::Result<bool> {
        match self.thumbnail_conditions[index] {
            Some(address) => Ok(self.game.vm().read(address)?.integer()? != 0),
            None => Ok(false),
        }
    }

    fn save_filename(&self, call: NativeCall) -> io::Result<Option<String>> {
        match self.arg(call, 0)? {
            Word::Integer(0) => Ok(None),
            value => {
                let bytes = self.game.vm().text(value.address()?)?;
                let name = encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(&bytes)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen save filename")
                    })?;
                let name = shinki_emu_lucifen_formats::member_key(&name)?;
                if name.contains('/') {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Lucifen save must be a filename",
                    ));
                }
                Ok(Some(name))
            }
        }
    }

    fn read_text_log(&mut self) -> io::Result<i32> {
        let Some(name) = &self.text_log_file else {
            return Ok(1);
        };
        let Some(data) = self.read_save_file(name)? else {
            return Ok(1);
        };
        match crate::state::TextLog::decode(data) {
            Ok(log) => {
                self.text_log = Some(log);
                Ok(0)
            }
            Err(_) => Ok(2),
        }
    }

    fn read_save_file(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        use std::io::Read;
        let path = self.system.save_root.join(name);
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        const MAX_BYTES: u64 = 64 * 1024 * 1024;
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut data)?;
        if data.len() as u64 > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Lucifen save exceeds its size limit",
            ));
        }
        Ok(Some(data))
    }

    fn read_global_save(&mut self) -> io::Result<i32> {
        let Some(name) = &self.global_save else {
            return Ok(1);
        };
        let Some(bytes) = self.read_save_file(name)? else {
            return Ok(1);
        };
        let (group, state) = match crate::state::decode_global(&bytes) {
            Ok(state) => state,
            Err(_) => return Ok(2),
        };
        Ok(
            if self.game.vm_mut().restore_variables(&state, group).is_ok() {
                0
            } else {
                2
            },
        )
    }

    fn write_global_save(&self) -> io::Result<i32> {
        let Some(name) = &self.global_save else {
            return Ok(2);
        };
        let state = self.game.vm().persistent_variables(2)?;
        if state.is_empty() {
            return Ok(2);
        }
        let data = crate::state::encode_global(&state, 2)?;
        self.write_save_file(name, &data)?;
        if let (Some(name), Some(log)) = (&self.text_log_file, &self.text_log) {
            self.write_save_file(name, log.bytes())?;
        }
        Ok(0)
    }

    fn write_save_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(&self.system.save_root)?;
        let mut file = tempfile::NamedTempFile::new_in(&self.system.save_root)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        file.persist(self.system.save_root.join(name))
            .map_err(|error| error.error)?;
        Ok(())
    }

    fn mount_archive(&mut self, call: NativeCall) -> io::Result<i32> {
        let address = self.arg(call, 0)?.address()?;
        let name = self.game.vm().text(address)?;
        let name = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(&name)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid CP932 archive name")
            })?;
        let key = self.archive_key.take();
        self.game.assets_mut().mount(&name, key)?;
        Ok(0)
    }

    fn string_call(&mut self, call: NativeCall) -> io::Result<Word> {
        let first = self.arg(call, 0)?.address()?;
        if call.function == 61 {
            return Ok(Word::Integer(self.game.vm().text(first)?.len() as i32));
        }
        let second = self.arg(call, 1)?.address()?;
        if matches!(call.function, 58 | 59) {
            let offset = self.arg(call, 2)?.integer()?;
            let limit = self.arg(call, 3)?.integer()?;
            let bytes = if call.function == 58 && limit <= 0 {
                Vec::new()
            } else {
                self.game
                    .vm()
                    .text(crate::vm::displace(second, i64::from(offset))?)?
            };
            let end = if call.function == 58 {
                (limit.max(0) as usize).min(bytes.len())
            } else {
                bytes
                    .iter()
                    .position(|&byte| byte == limit as u8)
                    .unwrap_or(bytes.len())
            };
            self.game.vm_mut().write_text(first, &bytes[..end])?;
            return Ok(Word::Integer(0));
        }
        let mut right = self.game.vm().text(second)?;
        if call.function == 57 {
            self.game.vm_mut().write_text(first, &right)?;
            return Ok(Word::Address(first));
        }
        let mut left = self.game.vm().text(first)?;
        if call.function == 60 {
            left.extend(right);
            self.game.vm_mut().write_text(first, &left)?;
            return Ok(Word::Address(first));
        }
        if call.function == 63 {
            left.make_ascii_lowercase();
            right.make_ascii_lowercase();
        }
        let value = match left.cmp(&right) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        Ok(Word::Integer(value))
    }

    fn change_text_case(&mut self, address: crate::Address, upper: bool) -> io::Result<()> {
        let mut bytes = self.game.vm().text(address)?;
        if bytes.is_ascii() {
            if upper {
                bytes.make_ascii_uppercase()
            } else {
                bytes.make_ascii_lowercase()
            }
            return self.game.vm_mut().write_text(address, &bytes);
        }
        if let Some(value) = self
            .text_encoding
            .decode_without_bom_handling_and_without_replacement(&bytes)
        {
            let value = if upper {
                value.to_uppercase()
            } else {
                value.to_lowercase()
            };
            let (converted, _, errors) = self.text_encoding.encode(&value);
            if !errors && converted.len() == bytes.len() {
                return self.game.vm_mut().write_text(address, &converted);
            }
        }
        Ok(())
    }

    fn environment_path(&mut self, call: NativeCall) -> io::Result<i32> {
        let selector = self.arg(call, 0)?.integer()?;
        let path = match selector {
            0 => {
                if self.app_build_id.is_none() {
                    self.app_build_id = Some(crate::app::build_id(&self.root)?);
                }
                self.app_build_id.as_deref().unwrap()
            }
            3 => &[],
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "unsupported Lucifen environment path selector",
                ));
            }
        };
        let length = self.arg(call, 2)?.integer()?;
        if length == 0 {
            return Ok(if path.is_empty() {
                0
            } else {
                path.len() as i32 + 1
            });
        }
        let count = usize::try_from(length).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Lucifen path buffer size",
            )
        })?;
        let destination = self.arg(call, 1)?.address()?;
        self.game
            .vm_mut()
            .write_text(destination, &path[..path.len().min(count - 1)])?;
        Ok(0)
    }

    fn arg(&self, call: NativeCall, index: usize) -> io::Result<Word> {
        self.game.vm().argument(call, index)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.audio_stop.store(true, Ordering::Release);
    }
}

fn packed_rgb(color: u32) -> [u8; 3] {
    let [blue, green, red, _] = color.to_le_bytes();
    [red, green, blue]
}

fn motion_number(value: &str) -> io::Result<i32> {
    let (digits, negative) = value
        .strip_prefix('-')
        .map_or((value, false), |digits| (digits, true));
    let (digits, radix) = if let Some(digits) = digits.strip_prefix("0x") {
        (digits, 16)
    } else if let Some(digits) = digits.strip_prefix("0b") {
        (digits, 2)
    } else {
        (digits, 10)
    };
    let value = u32::from_str_radix(digits, radix).map_err(io::Error::other)? as i32;
    Ok(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

fn align_scroll_text(
    layout: &LogicalTextLayout,
    bounds: [i32; 4],
    overhang: [i32; 4],
    mode: u8,
    flow: i32,
) -> ([i32; 2], i32) {
    let [left, top, right, bottom] = bounds;
    let [before_x, before_y, after_x, after_y] = overhang;
    let text_width = layout
        .lines
        .iter()
        .map(|line| spaced_line_width(layout, line, [flow, 0]).ceil() as i32)
        .max()
        .unwrap_or(0);
    let origin_x = left.saturating_sub(before_x);
    let end_x = right.saturating_add(after_x);
    let free_width = end_x.saturating_sub(origin_x).saturating_sub(text_width);
    let (x, justify) = match mode {
        0 | 1 => (origin_x.saturating_add(free_width / 2), 0),
        2 => (left, 0),
        3 => (end_x.saturating_sub(text_width), 0),
        _ => {
            let glyphs = layout
                .shaped_runs
                .iter()
                .map(|run| run.glyphs.len())
                .sum::<usize>();
            let gaps = (glyphs as i32).saturating_add(1);
            let spacing = free_width / gaps;
            (
                origin_x
                    .saturating_add(spacing)
                    .saturating_add((free_width % gaps) / 2),
                spacing,
            )
        }
    };
    let y = if mode == 0 || mode >= 4 {
        let start_y = top.saturating_sub(before_y);
        let free_height = bottom
            .saturating_add(after_y)
            .saturating_sub(start_y)
            .saturating_sub(layout.height.ceil() as i32);
        start_y.saturating_add(free_height / 2)
    } else {
        top
    };
    ([x, y], justify)
}

// Logical placement leaves the shaper's line metrics unchanged. Compute each
// scroll width as Lucifen did before: apply each tracking pass, then clamp.
fn spaced_line_width(layout: &LogicalTextLayout, line: &LayoutLine, spacing: [i32; 2]) -> f32 {
    if spacing == [0, 0] {
        return line.width;
    }
    let runs = &layout.shaped_runs;
    let first = runs.partition_point(|run| run.line < line.line);
    let gaps = runs[first..]
        .iter()
        .take_while(|run| run.line == line.line)
        .map(|run| run.glyphs.len())
        .sum::<usize>()
        .saturating_sub(1) as i32;
    spacing.into_iter().fold(line.width, |width, spacing| {
        (width + spacing.saturating_mul(gaps) as f32).max(0.0)
    })
}

fn slot_file_name(group: i32, slot: i32) -> Option<String> {
    ((0..=99).contains(&group) && (0..=99).contains(&slot))
        .then(|| format!("DATA{group:02}{slot:02}.SAV"))
}

fn slot_date(modified: std::time::SystemTime) -> String {
    let modified = time::OffsetDateTime::from(modified);
    let local = time::UtcOffset::local_offset_at(modified).unwrap_or(time::UtcOffset::UTC);
    let date = modified.to_offset(local);
    let day = ["日", "月", "火", "水", "木", "金", "土"]
        [date.weekday().number_days_from_sunday() as usize];
    format!(
        "{:04}/{:02}/{:02}({day}) {:02}:{:02}",
        date.year(),
        date.month() as u8,
        date.day(),
        date.hour(),
        date.minute()
    )
}

fn wrap_slot_summary(mut input: &[u8], chinese: bool) -> Vec<u8> {
    let mut output = Vec::new();
    let mut line = 0;
    let mut width = 0;
    while let Some(&first) = input.first().filter(|&&byte| byte != 0) {
        let double = if chinese {
            (0x81..=0xfe).contains(&first)
        } else {
            matches!(first, 0x81..=0x9f | 0xe0..=0xfc)
        };
        let length = if double && input.get(1).is_some_and(|&byte| byte != 0) {
            2
        } else {
            1
        };
        if first == b'\n' || width + length > 34 {
            line += 1;
            if line == 4 {
                break;
            }
            output.push(b'\n');
            width = 0;
            if first == b'\n' {
                input = &input[1..];
                continue;
            }
        }
        output.extend_from_slice(&input[..length]);
        input = &input[length..];
        width += length;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_text_positions_before_painting_straight_linear_edges() {
        let font = Font {
            family: "Noto Sans JP".into(),
            size: 24,
            line_spacing: 5,
            foreground: [209, 132, 17],
            shadow: [0; 3],
            effect: (0, 0, 0),
            glyph_capacity: 512,
            gradient: None,
            bold: false,
            italic: false,
        };
        let engine = Text::open(false).unwrap();
        let layout = engine.shape(&font, "OO\nOO".into(), 64).unwrap();
        let original = engine
            .rasterize(Arc::clone(&layout), [0, 0], [0, 0])
            .unwrap();
        assert_eq!(original.glyphs().count(), 4);
        let (_, [first_x, _]) = original.glyphs().next().unwrap();
        let origin = [-first_x as i32 - 2, 5];
        let spacing = [11, 0];
        let raster = engine
            .rasterize(Arc::clone(&layout), origin, spacing)
            .unwrap();
        for (index, ((_, [base_x, base_y]), (_, [x, y]))) in
            original.glyphs().zip(raster.glyphs()).enumerate()
        {
            let shift = if index % 2 == 0 { 0 } else { spacing[0] };
            assert_eq!(x, base_x + i64::from(origin[0] + shift));
            assert_eq!(y, base_y + i64::from(origin[1]));
        }
        let far = engine
            .rasterize(layout, [i32::MAX, 5], [i32::MIN, 0])
            .unwrap();
        let (_, [plain_x, plain_y]) = original.glyphs().nth(1).unwrap();
        let (_, [far_x, far_y]) = far.glyphs().nth(1).unwrap();
        assert_eq!([far_x, far_y], [plain_x - 1, plain_y + 5]);

        let (id, [x, y]) = raster.glyphs().nth(1).unwrap();
        let mask = &raster
            .resources()
            .iter()
            .find(|resource| resource.resource_id == id)
            .unwrap()
            .bitmap;
        assert_eq!(mask.format, roukaku_media_core::GlyphBitmapFormat::Alpha8);
        let (index, &coverage) = mask
            .pixels
            .as_slice()
            .iter()
            .enumerate()
            .find(|(_, alpha)| (32..=223).contains(*alpha))
            .unwrap();
        let x = x as usize + index % mask.width as usize;
        let y = y as usize + index / mask.width as usize;
        let offset = (y * 64 + x) * 4;

        let mut graphics = Graphics::open(64, 64).unwrap();
        for background in [[37, 61, 122, 117], [8, 12, 99, 0]] {
            let bitmap = graphics.create(64, 64).unwrap();
            assert!(graphics.fill(bitmap, None, background));
            graphics.draw_glyphs(bitmap, &font, &raster).unwrap();
            let surface = graphics.surface(bitmap).unwrap();
            assert_eq!([surface.width, surface.height], [64, 64]);
            let pixel = &surface.pixels()[offset..offset + 4];
            if background[3] == 0 {
                assert_eq!(pixel, &[209, 132, 17, coverage]);
            } else {
                let src = f64::from(coverage) / 255.0;
                let dst = f64::from(background[3]) / 255.0 * (1.0 - src);
                let decode = |byte: u8| ((f64::from(byte) / 255.0 + 0.055) / 1.055).powf(2.4);
                let expected = std::array::from_fn::<_, 3, _>(|channel| {
                    let linear = (src * decode(font.foreground[channel])
                        + dst * decode(background[channel]))
                        / (src + dst);
                    ((1.055 * linear.powf(1.0 / 2.4) - 0.055) * 255.0).round() as u8
                });
                assert_eq!(&pixel[..3], &expected);
                assert_eq!(pixel[3], ((src + dst) * 255.0).round() as u8);
            }
            assert_eq!(&surface.pixels()[..4], &background);
            assert_eq!(&surface.pixels()[surface.pixels().len() - 4..], &background);
            assert!(
                surface
                    .pixels()
                    .chunks_exact(64 * 4)
                    .any(|row| row[..4] != background)
            );
        }
    }
}
