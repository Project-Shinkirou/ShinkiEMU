use crate::{
    Error, Result,
    binary::{c_string, decode_japanese, dword, range, word},
    comments::SavedComments,
    effects::LensPosition,
    game::{find_directory, find_file, read_bounded},
    native_data::MUSIC_TRACK_LIMIT,
};
use encoding_rs::{Encoding, SHIFT_JIS};
use image::RgbaImage;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
struct Layout {
    header: usize,
    system_tail: usize,
    slot_size: usize,
    instruction: usize,
    preserve_view: usize,
    bank_shift: usize,
    classic: Option<ClassicScene>,
}

#[derive(Clone, Copy)]
struct ClassicScene {
    layer_bytes: usize,
    layers: usize,
    speaker_bytes: usize,
}

impl ClassicScene {
    fn tail_offset(self, original: usize) -> usize {
        original + self.layer_bytes * self.layers - LEGACY_LAYER_BYTES * 95
    }
}

const MODERN: Layout = Layout {
    header: 0x4b80,
    system_tail: 0x7f8,
    slot_size: 0x56fe4,
    instruction: 5533 * 4 - 19328,
    preserve_view: 66512 * 4 - 19328,
    bank_shift: 0,
    classic: None,
};
const LEGACY: Layout = Layout {
    header: 0x451c,
    system_tail: 0,
    slot_size: 0xd800,
    instruction: 4950 * 4 - 17692,
    preserve_view: 18245 * 4 - 17692,
    bank_shift: 36,
    classic: Some(ClassicScene {
        layer_bytes: LEGACY_LAYER_BYTES,
        layers: 95,
        speaker_bytes: 236,
    }),
};
const CLASSIC_EXPANDED: Layout = Layout {
    header: 0x4520,
    system_tail: 0x7f8,
    slot_size: 0x11af8,
    preserve_view: 22466 * 4 - 17696,
    bank_shift: 32,
    classic: Some(ClassicScene {
        layer_bytes: 432,
        layers: 120,
        speaker_bytes: 36,
    }),
    ..LEGACY
};

const SCENE_START: usize = MODERN.header;
const SCENE_SCRIPT: usize = 19336 - SCENE_START;
const SCENE_ASSETS: usize = 19496 - SCENE_START;
const ASSET_COUNT: usize = 51;
const ASSET_BYTES: usize = 48;
const SCENE_MUSIC: usize = 21944 - SCENE_START;
const SCENE_MUSIC_NUMBER: usize = 5494 * 4 - SCENE_START;
const SCENE_ENTRY: usize = 5535 * 4 - SCENE_START;
const SCENE_RESUME: usize = 5536 * 4 - SCENE_START;
const SCENE_VARIABLES: usize = 118892 - SCENE_START;
const SCENE_LAYERS: usize = 121248 - SCENE_START;
const LAYER_BYTES: usize = 960;
const LAYER_COUNT: usize = 150;
const LAYER_FIT_MODE: usize = 956;
// Fitted backgrounds store relative scale in units of 1/10,000.
const FIT_SCALE_UNIT: f64 = 10_000.0;
const SCENE_SPEAKER: usize = 265812 - SCENE_START;
const SCENE_CUSTOM_NAMES: usize = 66516 * 4 - SCENE_START;
const SCENE_GIVEN_NAME: usize = 265268 - SCENE_START;
const SCENE_FAMILY_NAME: usize = 265288 - SCENE_START;
const SCENE_TEXTURE_VARIANT: usize = 66508 * 4 - SCENE_START;
const SCENE_SPEAKER_BYTES: usize = SCENE_TEXTURE_VARIANT - SCENE_SPEAKER;
const SCENE_FOG_ENABLED: usize = 66336 * 4 - SCENE_START;
const SCENE_COMMENTS: usize = 266340 - SCENE_START;
const COMMENT_BYTES: usize = 82;
const COMMENT_LIMIT: usize = 20;
const SCENE_COMMENT_SETTINGS: usize = SCENE_COMMENTS + COMMENT_BYTES * COMMENT_LIMIT;
const SCENE_LINE: usize = 265552 - SCENE_START;
const SCENE_STAMP: usize = 475806 - 375652;
const SCENE_ANCHORS: usize = 6538 * 4 - SCENE_START;
const SCENE_ANCHOR_COUNT: usize = 26539 * 4 - SCENE_START;
const ANCHOR_BANKS: usize = 20;
const READ_SCENES: usize = 650;
const READ_INLINE: usize = 2636;
const READ_OVERFLOW_USED: usize = READ_INLINE + 20 * READ_SCENES;
const READ_OVERFLOW: usize = READ_OVERFLOW_USED + 4;
const READ_OVERFLOW_ENTRIES: usize = 500;
const MUSIC_FAVORITES: usize = 17720;
const LEGACY_ASSET_COUNT: usize = 34;
const LEGACY_ASSETS: usize = 17860 - LEGACY.header;
const LEGACY_MUSIC: usize = 19612 - LEGACY.header;
const LEGACY_VARIABLES: usize = 34868 - LEGACY.header;
const LEGACY_LAYERS: usize = 9306 * 4 - LEGACY.header;
const LEGACY_LAYER_BYTES: usize = 368;
const LEGACY_LINE: usize = 72484 - LEGACY.header;
const LEGACY_SPEAKER: usize = 72744 - LEGACY.header;
const LEGACY_FOG: usize = 18070 * 4 - LEGACY.header;
const LEGACY_LENS: usize = 18075 * 4 - LEGACY.header;
const SCENE_LENS: usize = 66342 * 4 - SCENE_START;
const LEGACY_GIVEN_NAME: usize = 72204 - LEGACY.header;
const LEGACY_FAMILY_NAME: usize = 72224 - LEGACY.header;
const LEGACY_STAMP: usize = 90754 - 72988;

impl Layout {
    fn system_size(self) -> usize {
        self.header + self.system_tail
    }
}

pub(crate) struct Saves {
    layout: Layout,
    encoding: &'static Encoding,
    system: Option<Vec<u8>>,
    persisted_system: Option<Vec<u8>>,
    preference_backup: Option<Vec<u8>>,
    system_path: Option<PathBuf>,
    slots: BTreeMap<u16, PathBuf>,
    scene_titles: Option<BTreeMap<String, String>>,
    directory: PathBuf,
    namespace: Option<String>,
    temporary_music_favorites: BTreeSet<u16>,
}

#[derive(Clone, Default)]
pub(crate) struct PlayerNames {
    pub(crate) given: String,
    pub(crate) family: String,
}

impl Saves {
    pub(crate) fn open(data: &Path, version: u16, encoding: &'static Encoding) -> Result<Self> {
        let layout = match version {
            0x9e97 => LEGACY,
            0x9f9a => CLASSIC_EXPANDED,
            _ => MODERN,
        };
        let directory = find_directory(data, "SAVEDATA")?;
        let mut store = Self {
            layout,
            encoding,
            system: None,
            persisted_system: None,
            preference_backup: None,
            system_path: None,
            slots: BTreeMap::new(),
            scene_titles: None,
            directory: directory.clone().unwrap_or_else(|| data.join("SAVEDATA")),
            namespace: None,
            temporary_music_favorites: BTreeSet::new(),
        };
        let system_path = match &directory {
            Some(directory) => find_file(directory, "system.dat")?,
            None => None,
        };
        if let Some(path) = system_path {
            store.system = Some(read_system(&path, layout)?);
            store.system_path = Some(path);
        } else if let Some(root) = data.parent()
            && let Some(mut value) = crate::native_data::system_template(root, layout.header)?
        {
            value.resize(layout.system_size(), 0);
            for (index, field) in [(92, 0u32), (93, 5), (94, 2), (95, 4)] {
                let offset = index * 4 - layout.bank_shift;
                value[offset..offset + 4].copy_from_slice(&field.to_le_bytes());
            }
            store.system = Some(value);
            let path = directory.clone().unwrap_or_else(|| data.join("SAVEDATA"));
            store.system_path = Some(path.join("system.dat"));
        }
        store.persisted_system.clone_from(&store.system);
        let root = data
            .parent()
            .ok_or(Error::Invalid("ADV32 game directory"))?;
        let declared_namespace = crate::native_data::save_namespace(root);
        let prefix_hint = declared_namespace.as_ref().ok().and_then(Option::as_deref);
        let mut groups = BTreeMap::<String, BTreeMap<u16, PathBuf>>::new();
        if let Some(directory) = directory {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                if !entry.file_type()?.is_file()
                    || entry.metadata()?.len() != layout.slot_size as u64
                {
                    continue;
                }
                let name = entry.file_name();
                let Some((prefix, slot)) = name
                    .to_str()
                    .and_then(|name| slot_filename(name, prefix_hint))
                else {
                    continue;
                };
                if groups
                    .entry(prefix.to_owned())
                    .or_default()
                    .insert(slot, entry.path())
                    .is_some()
                {
                    return Err(Error::Invalid("ambiguous ADV32 save slot"));
                }
            }
        }
        if groups.len() > 1 {
            return Err(Error::Invalid("multiple native ADV32 save identities"));
        }
        if let Some((name, slots)) = groups.pop_first() {
            store.namespace = Some(name);
            store.slots = slots;
        }
        if store.namespace.is_none() {
            store.namespace = declared_namespace?;
        }
        Ok(store)
    }

    pub(crate) fn exists(&self, slot: u16) -> bool {
        self.slots.contains_key(&slot)
    }

    pub(crate) fn player_names(&self) -> Result<PlayerNames> {
        let Some(system) = &self.system else {
            return Ok(PlayerNames::default());
        };
        let encoding = self.player_name_encoding()?;
        Ok(PlayerNames {
            given: saved_text(system, 200 - self.layout.bank_shift, 16, encoding)?,
            family: saved_text(system, 184 - self.layout.bank_shift, 16, encoding)?,
        })
    }

    pub(crate) fn uses_custom_player_names(&self, names: &PlayerNames) -> Result<bool> {
        let root = self
            .directory
            .parent()
            .and_then(Path::parent)
            .ok_or(Error::Invalid("ADV32 game directory"))?;
        let Some(template) = crate::native_data::system_template(root, self.layout.header)? else {
            return Ok(false);
        };
        let family = saved_string(&template, 184 - self.layout.bank_shift, 16)?;
        let given = saved_string(&template, 200 - self.layout.bank_shift, 16)?;
        Ok(names.family != family || names.given != given)
    }

    pub(crate) fn player_name_encoding(&self) -> Result<&'static Encoding> {
        let Some(system) = &self.system else {
            return Ok(SHIFT_JIS);
        };
        let family = saved_string(system, 184 - self.layout.bank_shift, 16);
        let given = saved_string(system, 200 - self.layout.bank_shift, 16);
        Ok(if family.is_ok() && given.is_ok() {
            SHIFT_JIS
        } else {
            self.encoding
        })
    }

    pub(crate) fn set_player_names(&mut self, names: &PlayerNames) -> Result<()> {
        let encoding = self.player_name_encoding()?;
        let mut encoded = [0; 32];
        put_text(&mut encoded, 0, 16, &names.family, encoding)?;
        put_text(&mut encoded, 16, 16, &names.given, encoding)?;
        let Some(system) = &mut self.system else {
            return Err(Error::Unsupported("initialization of ADV32 player names"));
        };
        let at = 184 - self.layout.bank_shift;
        system[at..at + encoded.len()].copy_from_slice(&encoded);
        self.flush_globals()
    }

    pub(crate) fn slot_caption(
        &mut self,
        slot: u16,
        preview: bool,
    ) -> Result<Option<(String, String)>> {
        let Some(path) = self.slots.get(&slot) else {
            return Ok(None);
        };
        let bytes = read_bounded(path, self.layout.slot_size)?;
        if bytes.len() != self.layout.slot_size {
            return Ok(None);
        }
        let (stamp, line_at) = match self.layout.classic {
            Some(scene) => (LEGACY_STAMP, scene.tail_offset(LEGACY_LINE)),
            None => (SCENE_STAMP, SCENE_LINE),
        };
        let [month, day, hour, minute, year, second] =
            <[u8; 6]>::try_from(range(&bytes, stamp, 6)?)
                .map_err(|_| Error::Invalid("ADV32 save timestamp"))?;
        let year = 2000 + u16::from(year);
        let date = if self.layout.classic.is_some() {
            format!("{year} {month}/{day} {hour}:{minute}:{second}")
        } else {
            format!("{year} {month:02}/{day:02} {hour:02}:{minute:02}:{second:02}")
        };
        if self.scene_titles.is_none() {
            let root = self
                .directory
                .parent()
                .and_then(Path::parent)
                .ok_or(Error::Invalid("ADV32 game directory"))?;
            self.scene_titles = Some(
                crate::native_data::save_scene_titles(root, self.encoding)?.unwrap_or_default(),
            );
        }
        let script = saved_string(&bytes, SCENE_SCRIPT, 160)?;
        let mut caption = self
            .scene_titles
            .as_ref()
            .and_then(|titles| titles.get(&script.to_ascii_lowercase()))
            .cloned()
            .unwrap_or(script);
        if preview {
            let line = saved_text(&bytes, line_at, 260, self.encoding)?;
            if !line.is_empty() {
                caption.push(' ');
                caption.push_str(&line);
            }
        }
        Ok(Some((date, caption)))
    }

    pub(crate) fn thumbnail(&self, slot: u16) -> Result<Option<RgbaImage>> {
        let Some(save) = self.slots.get(&slot) else {
            return Ok(None);
        };
        let Some(stem) = save.file_stem().and_then(|name| name.to_str()) else {
            return Ok(None);
        };
        let Some(path) = find_file(&self.directory, &format!("{stem}.png"))? else {
            return Ok(None);
        };
        let bytes = read_bounded(&path, 8 * 1024 * 1024)?;
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(2048);
        limits.max_image_height = Some(2048);
        limits.max_alloc = Some(20 * 1024 * 1024);
        reader.limits(limits);
        reader
            .decode()
            .map(|image| Some(image.into_rgba8()))
            .map_err(|error| Error::Image(error.to_string()))
    }

    pub(crate) fn was_read(&self, scene: usize, label: u16) -> Result<Option<bool>> {
        let Some(data) = &self.system else {
            return Ok(None);
        };
        if scene >= READ_SCENES || label == 0 {
            return Ok(None);
        }
        let entry = READ_INLINE - self.layout.bank_shift + 20 * scene;
        let overflow_at = READ_OVERFLOW - self.layout.bank_shift;
        let pointer = dword(data, entry)? as usize;
        let spans = read_spans(data, entry, pointer, overflow_at)?;
        Ok(Some(
            spans
                .iter()
                .any(|&(first, last)| (first..=last).contains(&label)),
        ))
    }

    /// Record a label and return whether it had already been reached.
    pub(crate) fn mark_read(&mut self, scene: usize, label: u16) -> Result<Option<bool>> {
        let Some(data) = &mut self.system else {
            return Ok(None);
        };
        if scene >= READ_SCENES || label == 0 {
            return Ok(None);
        }
        let inline = READ_INLINE - self.layout.bank_shift;
        let overflow_at = READ_OVERFLOW - self.layout.bank_shift;
        let entry = inline + 20 * scene;
        let pointer = dword(data, entry)? as usize;
        let mut spans = read_spans(data, entry, pointer, overflow_at)?;
        if spans
            .iter()
            .any(|&(first, last)| (first..=last).contains(&label))
        {
            return Ok(Some(true));
        }
        spans.push((label, label));
        spans.sort_unstable();
        let mut merged = Vec::<(u16, u16)>::new();
        for (first, last) in spans {
            if let Some(previous) = merged.last_mut()
                && first <= previous.1.saturating_add(1)
            {
                previous.1 = previous.1.max(last);
            } else {
                merged.push((first, last));
            }
        }
        if pointer == 0 && merged.len() <= 4 {
            write_spans(&mut data[entry + 4..entry + 20], &merged);
            return Ok(Some(false));
        }

        // Repack the bounded overflow lists before applying any writes so an
        // exhausted original save keeps its existing read history intact.
        let mut overflow = vec![(0, 0)];
        let mut pointers = Vec::new();
        for index in 0..READ_SCENES {
            let at = inline + 20 * index;
            let previous = dword(data, at)? as usize;
            if previous == 0 && index != scene {
                continue;
            }
            let start = overflow.len();
            if index == scene {
                overflow.extend_from_slice(&merged);
            } else {
                overflow.extend(read_spans(data, at, previous, overflow_at)?);
            }
            overflow.push((0, 0));
            pointers.push((at, start));
        }
        if overflow.len() <= READ_OVERFLOW_ENTRIES {
            let end = overflow_at + READ_OVERFLOW_ENTRIES * 4;
            write_spans(&mut data[overflow_at..end], &overflow);
            put_dword(data, overflow_at - 4, overflow.len() as u32)?;
            for (at, pointer) in pointers {
                put_dword(data, at, pointer as u32)?;
            }
        }
        Ok(Some(false))
    }

    pub(crate) fn global(&self, bank: u16, index: u16) -> Option<i32> {
        let data = self.system.as_deref()?;
        let field = global_field(self.layout, bank, index)?;
        Some(match field {
            Field::Byte(at) => i32::from(*data.get(at)?),
            Field::Word(at) => dword(data, at).ok()? as i32,
            Field::Inverse(at) => 99i32.wrapping_sub(dword(data, at).ok()? as i32),
        })
    }

    pub(crate) fn music_favorite(&self, track: u16) -> bool {
        if self.layout.classic.is_some() {
            return self.temporary_music_favorites.contains(&track);
        }
        self.system.as_ref().is_some_and(|system| {
            self.layout.bank_shift == 0
                && usize::from(track) < MUSIC_TRACK_LIMIT
                && system[MUSIC_FAVORITES + usize::from(track) * 4] != 0
        })
    }

    pub(crate) fn set_music_favorite(&mut self, track: u16, value: bool) {
        if self.layout.classic.is_some() && usize::from(track) < MUSIC_TRACK_LIMIT {
            if value {
                self.temporary_music_favorites.insert(track);
            } else {
                self.temporary_music_favorites.remove(&track);
            }
        }
        if let Some(system) = &mut self.system
            && self.layout.bank_shift == 0
            && usize::from(track) < MUSIC_TRACK_LIMIT
        {
            let offset = MUSIC_FAVORITES + usize::from(track) * 4;
            system[offset..offset + 4].copy_from_slice(&u32::from(value).to_le_bytes());
        }
    }

    pub(crate) fn update_global(&mut self, bank: u16, index: u16, value: i32) {
        if let Some(data) = &mut self.system
            && let Some(field) = global_field(self.layout, bank, index)
        {
            match field {
                Field::Byte(at) => data[at] = value as u8,
                Field::Word(at) => data[at..at + 4].copy_from_slice(&value.to_le_bytes()),
                Field::Inverse(at) => {
                    data[at..at + 4].copy_from_slice(&99i32.wrapping_sub(value).to_le_bytes())
                }
            }
        }
    }

    pub(crate) fn reload_globals(&mut self) -> Result<()> {
        if let Some(path) = &self.system_path
            && path.is_file()
        {
            self.system = Some(read_system(path, self.layout)?);
            self.persisted_system.clone_from(&self.system);
        }
        Ok(())
    }

    pub(crate) fn preference_mark(&mut self, mode: u8) -> Option<bool> {
        let system = self.system.as_mut()?;
        let header = &mut system[..self.layout.header];
        if mode == 0 {
            self.preference_backup = Some(header.to_vec());
        } else if let Some(initial) = self.preference_backup.take() {
            if mode == 1 {
                // The current settings page is UI state rather than a preference.
                let page = 216 + 54 - self.layout.bank_shift;
                return Some(
                    header[..page] != initial[..page] || header[page + 1..] != initial[page + 1..],
                );
            }
            header.copy_from_slice(&initial);
        }
        None
    }

    pub(crate) fn reset_preferences(&mut self, page: u8) -> Result<()> {
        let Some(system) = &mut self.system else {
            return Ok(());
        };
        let Some(root) = self.directory.parent().and_then(Path::parent) else {
            return Ok(());
        };
        let Some(defaults) = crate::native_data::system_template(root, self.layout.header)? else {
            return Ok(());
        };
        let fields: &[usize] = match page {
            0 => &[20, 9, 11, 24, 44, 35, 33, 25, 26, 45],
            1 => &[1, 6, 3, 5, 28, 8, 39, 27],
            2 => &[32, 31, 15, 13, 17, 16, 19, 18, 22, 37, 29, 30, 23],
            4 if self.layout.bank_shift == 0 => {
                system[17640..17720].fill(0);
                return Ok(());
            }
            5 => &[36, 38],
            _ => return Ok(()),
        };
        for &index in fields {
            let at = index * 4;
            system[at..at + 4].copy_from_slice(range(&defaults, at, 4)?);
        }
        if self.layout.bank_shift == 0 {
            if page == 1 {
                system[216 + 49] = 99u8.saturating_sub(dword(system, 24)? as u8);
            } else if page == 5 {
                for (index, value) in [(92, 0), (93, 5), (94, 2), (95, 4)] {
                    put_dword(system, index * 4, value)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn is_global(&self, bank: u16, index: u16) -> bool {
        global_field(self.layout, bank, index).is_some()
    }

    pub(crate) fn reset_settings(&mut self) -> Result<()> {
        let Some(system) = &mut self.system else {
            return Err(Error::Unsupported("original ADV32 settings"));
        };
        let root = self
            .directory
            .parent()
            .and_then(Path::parent)
            .ok_or(Error::Invalid("ADV32 game directory"))?;
        let defaults = crate::native_data::system_template(root, self.layout.header)?
            .ok_or(Error::Unsupported("original ADV32 settings defaults"))?;
        let settings_end = 184 - self.layout.bank_shift;
        system[..settings_end].copy_from_slice(&defaults[..settings_end]);
        if self.layout.classic.is_some() {
            for (index, value) in [(84, 0), (85, 5), (86, 2), (87, 4)] {
                put_dword(system, index * 4, value)?;
            }
            system[17608..17692].fill(0);
            self.temporary_music_favorites.clear();
        } else {
            for (index, value) in [(92, 0), (93, 5), (94, 2), (95, 4)] {
                put_dword(system, index * 4, value)?;
            }
            let favorites_end = MUSIC_FAVORITES + MUSIC_TRACK_LIMIT * 4;
            system[17640..favorites_end + 4].fill(0);
        }
        Ok(())
    }

    pub(crate) fn flush_pending(&mut self) -> Result<()> {
        if self.system != self.persisted_system {
            self.flush_globals()?;
        }
        Ok(())
    }

    pub(crate) fn flush_globals(&mut self) -> Result<()> {
        let (Some(data), Some(path)) = (&self.system, &self.system_path) else {
            return Err(Error::Unsupported(
                "initialization of a new ADV32 system.dat",
            ));
        };
        let parent = path
            .parent()
            .ok_or(Error::Invalid("ADV32 save directory"))?;
        ensure_save_directory(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(data)?;
        staged.as_file().sync_all()?;
        staged.persist(path).map_err(|error| error.error)?;
        self.persisted_system.clone_from(&self.system);
        Ok(())
    }

    pub(crate) fn read_slot(&self, slot: u16) -> Result<Option<NativeSlot>> {
        let Some(path) = self.slots.get(&slot) else {
            return Ok(None);
        };
        let bytes = read_bounded(path, self.layout.slot_size)?;
        if let Some(scene) = self.layout.classic {
            Ok(Some(NativeSlot::decode_legacy(
                &bytes,
                self.layout,
                scene,
                self.encoding,
            )?))
        } else {
            Ok(Some(NativeSlot::decode(&bytes, self.encoding)?))
        }
    }

    pub(crate) fn write_slot(
        &mut self,
        slot: u16,
        snapshot: &NativeSlot,
        thumbnail: &image::RgbaImage,
    ) -> Result<()> {
        let bytes = self.encode_slot(snapshot)?;
        let namespace = self
            .namespace
            .as_deref()
            .ok_or(Error::Unsupported("original ADV32 save namespace"))?;
        let directory = &self.directory;
        ensure_save_directory(directory)?;
        let path = match self.slots.get(&slot) {
            Some(path) => Cow::Borrowed(path.as_path()),
            None => {
                let name = format!("{namespace}{slot:02}.dat");
                // Files of an incompatible size are not indexed as slots. An
                // empty slot in the menu must not overwrite one of those files.
                if find_file(directory, &name)?.is_some() {
                    return Err(Error::Invalid("occupied ADV32 save destination"));
                }
                Cow::Owned(directory.join(name))
            }
        };
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;

        if self.layout.classic.is_none() {
            let picture =
                image::imageops::resize(thumbnail, 128, 72, image::imageops::FilterType::Triangle);
            let picture = image::DynamicImage::ImageRgba8(picture).into_rgb8();
            let png = tempfile::NamedTempFile::new_in(directory)?;
            picture
                .write_to(&mut png.as_file(), image::ImageFormat::Png)
                .map_err(|error| Error::Image(error.to_string()))?;
            png.as_file().sync_all()?;
            png.persist(path.with_extension("png"))
                .map_err(|error| error.error)?;
        }
        match path {
            Cow::Borrowed(path) => {
                staged.persist(path).map_err(|error| error.error)?;
            }
            Cow::Owned(path) => {
                staged
                    .persist_noclobber(&path)
                    .map_err(|error| error.error)?;
                self.slots.insert(slot, path);
            }
        }
        Ok(())
    }

    pub(crate) fn write_preliminary_slot(&self, slot: u16, snapshot: &NativeSlot) -> Result<()> {
        let namespace = self
            .namespace
            .as_deref()
            .ok_or(Error::Unsupported("original ADV32 save namespace"))?;
        let bytes = self.encode_slot(snapshot)?;
        ensure_save_directory(&self.directory)?;
        let mut staged = tempfile::NamedTempFile::new_in(&self.directory)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        staged
            .persist(self.directory.join(format!("PRE{namespace}{slot:02}.dat")))
            .map_err(|error| error.error)?;
        Ok(())
    }

    fn encode_slot(&self, snapshot: &NativeSlot) -> Result<Vec<u8>> {
        if let Some(scene) = self.layout.classic {
            snapshot.encode_legacy(self.layout, scene, self.encoding)
        } else {
            snapshot.encode(self.encoding)
        }
    }
}

fn ensure_save_directory(directory: &Path) -> Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if !metadata.is_dir() || metadata.is_symlink() => {
            Err(Error::Invalid("ADV32 save directory"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(fs::create_dir(directory)?)
        }
        Err(error) => Err(error.into()),
    }
}

fn read_spans(
    data: &[u8],
    at: usize,
    pointer: usize,
    overflow_at: usize,
) -> Result<Vec<(u16, u16)>> {
    let (start, count) = if pointer == 0 {
        (at + 4, 4)
    } else if pointer < READ_OVERFLOW_ENTRIES {
        (overflow_at + 4 * pointer, READ_OVERFLOW_ENTRIES - pointer)
    } else {
        return Err(Error::Invalid("ADV32 read-history index"));
    };
    let mut spans = Vec::new();
    for record in range(data, start, 4 * count)?.chunks_exact(4) {
        let first = word(record, 0)?;
        if first == 0 {
            break;
        }
        spans.push((first, word(record, 2)?));
    }
    Ok(spans)
}

fn write_spans(data: &mut [u8], spans: &[(u16, u16)]) {
    data.fill(0);
    for (record, &(first, last)) in data.chunks_exact_mut(4).zip(spans) {
        record[..2].copy_from_slice(&first.to_le_bytes());
        record[2..].copy_from_slice(&last.to_le_bytes());
    }
}

#[derive(Clone, Copy)]
pub(crate) struct NativeLayer {
    pub(crate) position: (i32, i32),
    pub(crate) extent: (u32, u32),
    pub(crate) fitted_extent: Option<(u32, u32)>,
    pub(crate) pivot: (i32, i32),
    pub(crate) scale: u32,
    pub(crate) visible: bool,
    pub(crate) order: u8,
}

#[derive(Clone, Default)]
pub(crate) struct NativeSlot {
    pub(crate) script: String,
    pub(crate) instruction: usize,
    pub(crate) resume: usize,
    pub(crate) entry: u16,
    pub(crate) entry_step: u16,
    pub(crate) assets: BTreeMap<u8, String>,
    pub(crate) music: Option<(u16, String)>,
    pub(crate) speaker: String,
    pub(crate) line: String,
    pub(crate) variables: BTreeMap<(u16, u16), i32>,
    pub(crate) layers: BTreeMap<usize, NativeLayer>,
    pub(crate) anchors: BTreeMap<(u16, String), (i32, i32)>,
    pub(crate) texture_variant: u16,
    pub(crate) preserve_view: bool,
    pub(crate) fog_enabled: bool,
    pub(crate) comments: SavedComments,
    pub(crate) player_names: PlayerNames,
    pub(crate) custom_names: bool,
    pub(crate) lens_position: LensPosition,
    pub(crate) lens_strength: u8,
}

impl NativeSlot {
    fn decode_legacy(
        bytes: &[u8],
        layout: Layout,
        scene: ClassicScene,
        encoding: &'static Encoding,
    ) -> Result<Self> {
        if bytes.len() != layout.slot_size {
            return Err(Error::Invalid("ADV32 legacy save slot layout"));
        }
        let instruction = layout.instruction;
        let mut layers = BTreeMap::new();
        for (local, native) in [(0, 0), (10, 10), (20, 20), (30, 30), (40, 40), (63, 51)] {
            let layer = range(
                bytes,
                LEGACY_LAYERS + native * scene.layer_bytes,
                scene.layer_bytes,
            )?;
            let number = |at| f32::from_bits(dword(layer, at).unwrap_or_default());
            layers.insert(
                local,
                NativeLayer {
                    position: (number(0).round() as i32, number(4).round() as i32),
                    extent: (number(112) as u32, number(116) as u32),
                    fitted_extent: None,
                    pivot: (number(96).round() as i32, number(100).round() as i32),
                    scale: number(84).round() as u32,
                    visible: number(80) > 0.0,
                    order: dword(layer, 364)?.min(100) as u8,
                },
            );
        }
        let music = saved_string(bytes, LEGACY_MUSIC, 32)?;
        Ok(Self {
            script: saved_string(bytes, SCENE_SCRIPT, 160)?,
            instruction: dword(bytes, instruction)? as usize,
            resume: dword(bytes, instruction + 12)? as usize,
            entry: dword(bytes, instruction + 8)? as u16,
            entry_step: dword(bytes, instruction + 4)? as u16,
            assets: read_asset_table(bytes, LEGACY_ASSETS, LEGACY_ASSET_COUNT)?,
            music: if music.is_empty() {
                None
            } else {
                Some((dword(bytes, LEGACY_MUSIC + 32)? as u16, music))
            },
            speaker: saved_text(
                bytes,
                scene.tail_offset(LEGACY_SPEAKER),
                scene.speaker_bytes,
                encoding,
            )?,
            line: saved_text(bytes, scene.tail_offset(LEGACY_LINE), 260, encoding)?,
            variables: read_variables(bytes, LEGACY_VARIABLES)?,
            layers,
            anchors: BTreeMap::new(),
            texture_variant: 0,
            preserve_view: dword(bytes, layout.preserve_view)? != 0,
            fog_enabled: dword(bytes, scene.tail_offset(LEGACY_FOG))? != 0,
            lens_position: read_lens_position(bytes, scene.tail_offset(LEGACY_LENS))?,
            lens_strength: dword(bytes, scene.tail_offset(LEGACY_LENS))? as u8,
            comments: SavedComments::default(),
            player_names: PlayerNames {
                given: saved_text(bytes, scene.tail_offset(LEGACY_GIVEN_NAME), 20, encoding)?,
                family: saved_text(bytes, scene.tail_offset(LEGACY_FAMILY_NAME), 20, encoding)?,
            },
            custom_names: false,
        })
    }

    fn encode_legacy(
        &self,
        layout: Layout,
        scene: ClassicScene,
        encoding: &'static Encoding,
    ) -> Result<Vec<u8>> {
        let mut data = vec![0; layout.slot_size];
        let instruction = layout.instruction;
        put_string(&mut data, SCENE_SCRIPT, 160, &self.script)?;
        put_dword(&mut data, instruction, self.instruction as u32)?;
        put_dword(&mut data, instruction + 4, self.entry_step.into())?;
        put_dword(&mut data, instruction + 8, self.entry.into())?;
        put_dword(&mut data, instruction + 12, self.resume as u32)?;
        put_dword(&mut data, layout.preserve_view, self.preserve_view.into())?;
        write_asset_table(&mut data, LEGACY_ASSETS, LEGACY_ASSET_COUNT, &self.assets)?;
        if let Some((number, name)) = &self.music {
            put_string(&mut data, LEGACY_MUSIC, 32, name)?;
            put_dword(&mut data, LEGACY_MUSIC + 32, (*number).into())?;
        }
        put_text(
            &mut data,
            scene.tail_offset(LEGACY_SPEAKER),
            scene.speaker_bytes,
            &self.speaker,
            encoding,
        )?;
        put_text(
            &mut data,
            scene.tail_offset(LEGACY_LINE),
            260,
            &self.line,
            encoding,
        )?;
        put_text(
            &mut data,
            scene.tail_offset(LEGACY_GIVEN_NAME),
            20,
            &self.player_names.given,
            encoding,
        )?;
        put_text(
            &mut data,
            scene.tail_offset(LEGACY_FAMILY_NAME),
            20,
            &self.player_names.family,
            encoding,
        )?;
        put_dword(
            &mut data,
            scene.tail_offset(LEGACY_FOG),
            self.fog_enabled.into(),
        )?;
        write_lens_position(
            &mut data,
            scene.tail_offset(LEGACY_LENS),
            self.lens_strength,
            &self.lens_position,
        )?;
        write_variables(&mut data, LEGACY_VARIABLES, &self.variables);
        for index in 0..scene.layers {
            let key = if index == 51 { 63 } else { index };
            let layer = self.layers.get(&key).copied().unwrap_or(NativeLayer {
                position: (0, 0),
                extent: (0, 0),
                fitted_extent: None,
                pivot: (0, 0),
                scale: 100,
                visible: false,
                order: 50,
            });
            let at = LEGACY_LAYERS + index * scene.layer_bytes;
            let (x, y) = (layer.position.0 as f32, layer.position.1 as f32);
            let (w, h) = (layer.extent.0 as f32, layer.extent.1 as f32);
            for (offset, value) in [
                (0, x),
                (4, y),
                (24, x),
                (28, y),
                (32, w),
                (36, h),
                (72, w),
                (76, h),
                (80, f32::from(u8::from(layer.visible))),
                (84, layer.scale as f32),
                (88, layer.scale as f32),
                (96, layer.pivot.0 as f32),
                (100, layer.pivot.1 as f32),
                (112, w),
                (116, h),
                (120, w),
                (124, h),
            ] {
                put_dword(&mut data, at + offset, value.to_bits())?;
            }
            put_dword(&mut data, at + 128, 1)?;
            put_dword(&mut data, at + 364, layer.order.into())?;
        }
        write_timestamp(&mut data, LEGACY_STAMP);
        Ok(data)
    }

    fn decode(bytes: &[u8], encoding: &'static Encoding) -> Result<Self> {
        if bytes.len() != MODERN.slot_size {
            return Err(Error::Invalid("ADV32 save slot layout"));
        }
        let assets = read_asset_table(bytes, SCENE_ASSETS, ASSET_COUNT)?;
        let music = saved_string(bytes, SCENE_MUSIC, 32)?;
        let variables = read_variables(bytes, SCENE_VARIABLES)?;
        let mut layers = BTreeMap::new();
        for index in [0, 10, 20, 30, 40, 50, 63] {
            let bytes = range(bytes, SCENE_LAYERS + index * LAYER_BYTES, LAYER_BYTES)?;
            let number = |offset| f32::from_bits(dword(bytes, offset).unwrap_or_default());
            let width = number(64) as u32;
            let height = number(68) as u32;
            let fitted_extent = if index == 0 && dword(bytes, LAYER_FIT_MODE)? != 0 {
                let fit = |length: u32, scale: f32| {
                    (f64::from(length) * f64::from(scale) / FIT_SCALE_UNIT).round() as u32
                };
                Some((fit(width, number(120)), fit(height, number(124))))
            } else {
                None
            };
            layers.insert(
                index,
                NativeLayer {
                    position: (number(0).round() as i32, number(4).round() as i32),
                    extent: (width, height),
                    fitted_extent,
                    pivot: (number(144).round() as i32, number(148).round() as i32),
                    scale: number(120).round() as u32,
                    visible: number(104) > 0.0,
                    order: dword(bytes, 904)?.min(100) as u8,
                },
            );
        }
        let mut anchors = BTreeMap::new();
        for actor in 0..ANCHOR_BANKS {
            let count = dword(bytes, SCENE_ANCHOR_COUNT + 4 * actor)? as usize;
            if count > 100 {
                return Err(Error::Invalid("ADV32 actor anchor count"));
            }
            for index in 0..count {
                let at = SCENE_ANCHORS + 4000 * actor + 40 * index;
                let name = saved_string(bytes, at, 32)?;
                if !name.is_empty() {
                    anchors.insert(
                        (actor as u16, name),
                        (dword(bytes, at + 32)? as i32, dword(bytes, at + 36)? as i32),
                    );
                }
            }
        }
        let mut comments = SavedComments {
            period: u16::try_from(dword(bytes, SCENE_COMMENT_SETTINGS)?).ok(),
            random_order: dword(bytes, SCENE_COMMENT_SETTINGS + 4)? != 0,
            clear_every: dword(bytes, SCENE_COMMENT_SETTINGS + 8)? as u16,
            period_scale: [
                dword(bytes, SCENE_COMMENT_SETTINGS + 12)? as i32,
                dword(bytes, SCENE_COMMENT_SETTINGS + 16)? as i32,
                dword(bytes, SCENE_COMMENT_SETTINGS + 20)? as i32,
            ],
            visible: dword(bytes, SCENE_COMMENT_SETTINGS + 24)? != 0,
            ..SavedComments::default()
        };
        for index in 0..COMMENT_LIMIT {
            let at = SCENE_COMMENTS + index * COMMENT_BYTES;
            let raw = c_string(range(bytes, at, COMMENT_BYTES)?)?;
            if raw.is_empty() {
                break;
            }
            comments.entries.push(raw.to_vec());
        }
        Ok(Self {
            script: saved_string(bytes, SCENE_SCRIPT, 160)?,
            instruction: dword(bytes, MODERN.instruction)? as usize,
            resume: dword(bytes, SCENE_RESUME)? as usize,
            entry: dword(bytes, SCENE_ENTRY)? as u16,
            entry_step: dword(bytes, SCENE_ENTRY - 4)? as u16,
            assets,
            music: if music.is_empty() {
                None
            } else {
                Some((dword(bytes, SCENE_MUSIC_NUMBER)? as u16, music))
            },
            speaker: saved_text(bytes, SCENE_SPEAKER, SCENE_SPEAKER_BYTES, encoding)?,
            line: saved_text(bytes, SCENE_LINE, 260, encoding)?,
            variables,
            layers,
            anchors,
            texture_variant: dword(bytes, SCENE_TEXTURE_VARIANT)? as u16,
            preserve_view: dword(bytes, MODERN.preserve_view)? != 0,
            fog_enabled: dword(bytes, SCENE_FOG_ENABLED)? != 0,
            lens_position: read_lens_position(bytes, SCENE_LENS)?,
            lens_strength: dword(bytes, SCENE_LENS)? as u8,
            comments,
            player_names: PlayerNames {
                given: saved_text(bytes, SCENE_GIVEN_NAME, 20, encoding)?,
                family: saved_text(bytes, SCENE_FAMILY_NAME, 20, encoding)?,
            },
            custom_names: dword(bytes, SCENE_CUSTOM_NAMES)? != 0,
        })
    }

    fn encode(&self, encoding: &'static Encoding) -> Result<Vec<u8>> {
        let mut data = vec![0; MODERN.slot_size];
        put_string(&mut data, SCENE_SCRIPT, 160, &self.script)?;
        put_dword(&mut data, MODERN.instruction, self.instruction as u32)?;
        put_dword(&mut data, SCENE_RESUME, self.resume as u32)?;
        put_dword(&mut data, MODERN.preserve_view, self.preserve_view.into())?;
        put_dword(&mut data, SCENE_ENTRY, u32::from(self.entry))?;
        put_dword(&mut data, SCENE_ENTRY - 4, u32::from(self.entry_step))?;
        write_asset_table(&mut data, SCENE_ASSETS, ASSET_COUNT, &self.assets)?;
        if let Some((number, name)) = &self.music {
            put_string(&mut data, SCENE_MUSIC, 32, name)?;
            put_dword(&mut data, SCENE_MUSIC_NUMBER, u32::from(*number))?;
        }
        put_text(
            &mut data,
            SCENE_SPEAKER,
            SCENE_SPEAKER_BYTES,
            &self.speaker,
            encoding,
        )?;
        put_text(&mut data, SCENE_LINE, 260, &self.line, encoding)?;
        put_dword(&mut data, SCENE_CUSTOM_NAMES, self.custom_names.into())?;
        put_text(
            &mut data,
            SCENE_GIVEN_NAME,
            20,
            &self.player_names.given,
            encoding,
        )?;
        put_text(
            &mut data,
            SCENE_FAMILY_NAME,
            20,
            &self.player_names.family,
            encoding,
        )?;
        put_dword(
            &mut data,
            SCENE_TEXTURE_VARIANT,
            self.texture_variant.into(),
        )?;
        put_dword(&mut data, SCENE_FOG_ENABLED, self.fog_enabled.into())?;
        write_lens_position(
            &mut data,
            SCENE_LENS,
            self.lens_strength,
            &self.lens_position,
        )?;
        for (index, raw) in self.comments.entries.iter().take(COMMENT_LIMIT).enumerate() {
            if raw.len() >= COMMENT_BYTES {
                return Err(Error::Invalid("ADV32 save comment length"));
            }
            let at = SCENE_COMMENTS + index * COMMENT_BYTES;
            data[at..at + raw.len()].copy_from_slice(raw);
        }
        put_dword(
            &mut data,
            SCENE_COMMENT_SETTINGS,
            self.comments.period.map_or(u32::MAX, u32::from),
        )?;
        put_dword(
            &mut data,
            SCENE_COMMENT_SETTINGS + 4,
            self.comments.random_order.into(),
        )?;
        put_dword(
            &mut data,
            SCENE_COMMENT_SETTINGS + 8,
            self.comments.clear_every.into(),
        )?;
        for (index, value) in self.comments.period_scale.into_iter().enumerate() {
            put_dword(
                &mut data,
                SCENE_COMMENT_SETTINGS + 12 + 4 * index,
                value as u32,
            )?;
        }
        put_dword(
            &mut data,
            SCENE_COMMENT_SETTINGS + 24,
            self.comments.visible.into(),
        )?;
        write_variables(&mut data, SCENE_VARIABLES, &self.variables);
        let mut counts = [0usize; ANCHOR_BANKS];
        for (&(actor, ref name), &(x, y)) in &self.anchors {
            let actor = usize::from(actor);
            let count = counts
                .get_mut(actor)
                .ok_or(Error::Invalid("ADV32 save actor number"))?;
            if *count >= 100 {
                return Err(Error::Invalid("ADV32 save actor anchor count"));
            }
            let at = SCENE_ANCHORS + 4000 * actor + 40 * *count;
            put_string(&mut data, at, 32, name)?;
            put_dword(&mut data, at + 32, x as u32)?;
            put_dword(&mut data, at + 36, y as u32)?;
            *count += 1;
        }
        for (actor, count) in counts.into_iter().enumerate() {
            put_dword(&mut data, SCENE_ANCHOR_COUNT + actor * 4, count as u32)?;
        }
        for index in 0..LAYER_COUNT {
            encode_layer(&mut data, index, self.layers.get(&index).copied())?;
        }
        write_timestamp(&mut data, SCENE_STAMP);
        Ok(data)
    }
}

fn read_asset_table(bytes: &[u8], at: usize, count: usize) -> Result<BTreeMap<u8, String>> {
    let mut assets = BTreeMap::new();
    for index in 0..count {
        let name = saved_string(bytes, at + index * ASSET_BYTES, ASSET_BYTES)?;
        if !name.is_empty() {
            assets.insert(index as u8, name);
        }
    }
    Ok(assets)
}

fn read_lens_position(bytes: &[u8], at: usize) -> Result<LensPosition> {
    Ok(LensPosition {
        center: (dword(bytes, at + 4)? as i32, dword(bytes, at + 8)? as i32),
        angles: (dword(bytes, at + 12)? as i32, dword(bytes, at + 16)? as i32),
        duration: std::time::Duration::from_micros(
            u64::from(dword(bytes, at + 20)?) * 1_000_000 / 60,
        ),
    })
}

fn write_lens_position(
    data: &mut [u8],
    at: usize,
    strength: u8,
    position: &LensPosition,
) -> Result<()> {
    let frames = (position.duration.as_nanos() * 60).div_ceil(1_000_000_000) as u32;
    for (index, value) in [
        u32::from(strength),
        position.center.0 as u32,
        position.center.1 as u32,
        position.angles.0 as u32,
        position.angles.1 as u32,
        frames,
    ]
    .into_iter()
    .enumerate()
    {
        put_dword(data, at + index * 4, value)?;
    }
    Ok(())
}

fn write_asset_table(
    data: &mut [u8],
    at: usize,
    count: usize,
    assets: &BTreeMap<u8, String>,
) -> Result<()> {
    for (&index, name) in assets {
        let index = usize::from(index);
        if index >= count {
            return Err(Error::Invalid("ADV32 save texture number"));
        }
        put_string(data, at + index * ASSET_BYTES, ASSET_BYTES, name)?;
    }
    Ok(())
}

fn read_variables(bytes: &[u8], at: usize) -> Result<BTreeMap<(u16, u16), i32>> {
    Ok(range(bytes, at, 1100)?
        .iter()
        .enumerate()
        .filter(|&(_, &value)| value != 0)
        .map(|(index, &value)| {
            (
                ((index / 100) as u16, (index % 100) as u16),
                i32::from(value),
            )
        })
        .collect())
}

fn write_variables(data: &mut [u8], at: usize, values: &BTreeMap<(u16, u16), i32>) {
    for (&(bank, index), &value) in values {
        if bank <= 10 && index < 100 {
            data[at + 100 * usize::from(bank) + usize::from(index)] = value as u8;
        }
    }
}

fn write_timestamp(data: &mut [u8], at: usize) {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    data[at..at + 6].copy_from_slice(&[
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.year().rem_euclid(100) as u8,
        now.second(),
    ]);
}

fn encode_layer(data: &mut [u8], index: usize, layer: Option<NativeLayer>) -> Result<()> {
    let at = SCENE_LAYERS + LAYER_BYTES * index;
    for field in [6, 7, 8, 26, 27, 28] {
        put_dword(data, at + field * 4, 1.0f32.to_bits())?;
    }
    for (field, value) in [(20, 1280), (21, 720), (44, 1)] {
        put_dword(data, at + field * 4, value)?;
    }
    let layer = layer.unwrap_or(NativeLayer {
        position: (0, 0),
        extent: (0, 0),
        fitted_extent: None,
        pivot: (0, 0),
        scale: 100,
        visible: false,
        order: 50,
    });
    put_dword(data, at + 904, layer.order.into())?;
    put_dword(
        data,
        at + LAYER_FIT_MODE,
        u32::from(layer.fitted_extent.is_some()),
    )?;
    let (x, y) = (layer.position.0 as f32, layer.position.1 as f32);
    let (w, h) = (layer.extent.0 as f32, layer.extent.1 as f32);
    let (scale_x, scale_y) = if let Some((width, height)) = layer.fitted_extent {
        let scale = |source: u32, destination: u32| {
            (f64::from(destination) * FIT_SCALE_UNIT / f64::from(source.max(1))).floor() as f32
        };
        (scale(layer.extent.0, width), scale(layer.extent.1, height))
    } else {
        (layer.scale as f32, layer.scale as f32)
    };
    for (field, value) in [
        (0, x),
        (1, y),
        (12, x),
        (13, y),
        (14, layer.extent.0.next_power_of_two() as f32),
        (15, layer.extent.1.next_power_of_two() as f32),
        (16, w),
        (17, h),
        (24, w),
        (25, h),
        (26, f32::from(u8::from(layer.visible))),
        (30, scale_x),
        (31, scale_y),
        (36, layer.pivot.0 as f32),
        (37, layer.pivot.1 as f32),
        (38, 1280.0),
        (39, 720.0),
    ] {
        put_dword(data, at + field * 4, value.to_bits())?;
    }
    Ok(())
}

fn saved_string(bytes: &[u8], offset: usize, length: usize) -> Result<String> {
    decode_japanese(c_string(range(bytes, offset, length)?)?)
}

fn saved_text(
    bytes: &[u8],
    offset: usize,
    length: usize,
    encoding: &'static Encoding,
) -> Result<String> {
    encoding
        .decode_without_bom_handling_and_without_replacement(c_string(range(
            bytes, offset, length,
        )?)?)
        .map(std::borrow::Cow::into_owned)
        .ok_or(Error::Invalid("ADV32 saved text encoding"))
}

fn put_string(bytes: &mut [u8], at: usize, length: usize, value: &str) -> Result<()> {
    put_text(bytes, at, length, value, SHIFT_JIS)
}

fn put_text(
    bytes: &mut [u8],
    at: usize,
    length: usize,
    value: &str,
    encoding: &'static Encoding,
) -> Result<()> {
    let (encoded, _, invalid) = encoding.encode(value);
    if invalid || encoded.len() >= length || encoded.contains(&0) {
        return Err(Error::Invalid("ADV32 native save string"));
    }
    let out = bytes
        .get_mut(at..at + length)
        .ok_or(Error::Invalid("ADV32 save string range"))?;
    out.fill(0);
    out[..encoded.len()].copy_from_slice(&encoded);
    Ok(())
}

fn put_dword(bytes: &mut [u8], at: usize, value: u32) -> Result<()> {
    bytes
        .get_mut(at..at + 4)
        .ok_or(Error::Invalid("ADV32 save integer range"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn read_system(path: &Path, layout: Layout) -> Result<Vec<u8>> {
    let data = read_bounded(path, layout.system_size())?;
    if data.len() != layout.system_size() {
        return Err(Error::Invalid("ADV32 system.dat layout"));
    }
    Ok(data)
}

enum Field {
    Byte(usize),
    Word(usize),
    Inverse(usize),
}

fn global_field(layout: Layout, bank: u16, index: u16) -> Option<Field> {
    let at = usize::from(index);
    match bank {
        93 if at < 20 => Some(Field::Word((4410 + at) * 4 - layout.bank_shift)),
        96 if at < 400 => Some(Field::Byte(384 + at - layout.bank_shift)),
        97 if at < 1200 => Some(Field::Byte(784 + at - layout.bank_shift)),
        98 if at < READ_SCENES => Some(Field::Byte(1984 + at - layout.bank_shift)),
        100 if at <= 0x95 => Some(Field::Byte(216 + at - layout.bank_shift)),
        99 if index == 11 => Some(Field::Inverse(6 * 4)),
        99 if layout.classic.is_some() => {
            let offset = match index {
                194 if layout.bank_shift == 32 => 37,
                196..=199 => 291 - at - layout.bank_shift / 4,
                200 => 36,
                201 => 35,
                202 => 34,
                203 => 30,
                205 => 32,
                221 if layout.bank_shift == 32 => 4422,
                225 if layout.bank_shift == 32 => 31,
                227 if layout.bank_shift == 32 => 33,
                _ => return common_global_word(layout, index),
            };
            Some(Field::Word(offset * 4))
        }
        99 => common_global_word(layout, index),
        _ => None,
    }
}

fn common_global_word(layout: Layout, index: u16) -> Option<Field> {
    let field = match index {
        5 => 18,
        7 => 15,
        8 => 17,
        9 => 19,
        10 => 9,
        12 => 11,
        13 => 4,
        14 => 13,
        15 => 16,
        16 => 7,
        17 => 12,
        18 => 10,
        26 => 1,
        27 => 2,
        28 => 5,
        29 => 3,
        30 => 8,
        180 => 45,
        181 => 44,
        182 => 43,
        184 => 42,
        185 => 41,
        188 => 40,
        190 => 30,
        191 => 39,
        194 => 38,
        196 => 95,
        197 => 94,
        198 => 93,
        199 => 92,
        200 => 37,
        201 => 36,
        202 => 35,
        203 => 31,
        205 => 33,
        206 => 29,
        207 => 22,
        208 => 23,
        209 => 28,
        210 => 27,
        211 => 26,
        212 => 25,
        213 => 24,
        214 => 21,
        221 => 4830,
        225 => 32,
        227 => 34,
        237 => 20,
        _ => return None,
    };
    // The fixed preference words end before the player names in classic slots.
    if layout.classic.is_some() && field * 4 >= 184 - layout.bank_shift {
        return None;
    }
    (field < layout.header / 4).then_some(Field::Word(field * 4))
}

fn slot_filename<'a>(name: &'a str, prefix_hint: Option<&str>) -> Option<(&'a str, u16)> {
    let (stem, ext) = name.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case("dat")
        || stem
            .as_bytes()
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"PRE"))
    {
        return None;
    }
    // The original %02d suffix can follow a namespace that itself ends in digits.
    if let Some(hint) = prefix_hint
        && let Some(prefix) = stem.get(..hint.len())
        && prefix.eq_ignore_ascii_case(hint)
    {
        let digits = &stem[hint.len()..];
        if (2..=3).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Some((prefix, digits.parse().ok()?));
        }
    }
    let suffix = stem
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if !(2..=3).contains(&suffix) {
        return None;
    }
    let (prefix, index) = stem.split_at(stem.len() - suffix);
    if prefix.is_empty() {
        return None;
    }
    Some((prefix, index.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_slot_parsing_preserves_digits_in_the_namespace() {
        assert_eq!(
            slot_filename("TEST400.dat", Some("test4")),
            Some(("TEST4", 0))
        );
        assert_eq!(
            slot_filename("TEST4100.dat", Some("test4")),
            Some(("TEST4", 100))
        );
        assert_eq!(slot_filename("TEST00.dat", None), Some(("TEST", 0)));
    }

    #[test]
    fn native_checkpoint_preserves_a_named_anchor_for_actor_banks_above_four() {
        let snapshot = NativeSlot {
            script: "PROLOGUE".into(),
            instruction: 320,
            resume: 300,
            entry: 4,
            entry_step: 2,
            assets: BTreeMap::from([(1, "BG\\ROOM".into())]),
            music: None,
            speaker: String::new(),
            line: String::new(),
            variables: BTreeMap::from([((3, 17), 8)]),
            layers: BTreeMap::new(),
            anchors: BTreeMap::from([((11, "POSE".into()), (146, 291))]),
            texture_variant: 0,
            preserve_view: true,
            fog_enabled: false,
            comments: SavedComments::default(),
            player_names: PlayerNames::default(),
            custom_names: false,
            lens_position: LensPosition::default(),
            lens_strength: 0,
        };
        let bytes = snapshot.encode(SHIFT_JIS).unwrap();
        assert_eq!(dword(&bytes, SCENE_ANCHOR_COUNT + 11 * 4).unwrap(), 1);
        let mut actual = NativeSlot::decode(&bytes, SHIFT_JIS).unwrap();
        assert!(actual.preserve_view);
        assert_eq!(actual.anchors, snapshot.anchors);
        assert_eq!(actual.assets, snapshot.assets);
        assert_eq!(actual.resume, 300);
        // The original writer stores the six date fields in this variable bank.
        for index in 90..96 {
            actual.variables.remove(&(5, index));
        }
        assert_eq!(actual.variables, snapshot.variables);
    }

    #[test]
    fn additional_read_ranges_preserve_existing_scene_lists() {
        for (version, layout) in [
            (0xabb6, MODERN),
            (0x9f9a, CLASSIC_EXPANDED),
            (0x9e97, LEGACY),
        ] {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join("SAVEDATA");
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("system.dat"), vec![0; layout.system_size()]).unwrap();
            let mut saves = Saves::open(root.path(), version, SHIFT_JIS).unwrap();
            for scene in [3, 5] {
                for label in [1, 3, 5, 7, 9] {
                    assert_eq!(saves.mark_read(scene, label).unwrap(), Some(false));
                }
            }
            assert_eq!(saves.mark_read(3, 2).unwrap(), Some(false));
            saves.flush_globals().unwrap();
            let saves = Saves::open(root.path(), version, SHIFT_JIS).unwrap();
            assert_eq!(saves.was_read(3, 2).unwrap(), Some(true));
            assert_eq!(saves.was_read(5, 2).unwrap(), Some(false));
            let data = saves.system.unwrap();
            let spans = |scene: usize| {
                let at = READ_INLINE - layout.bank_shift + 20 * scene;
                read_spans(
                    &data,
                    at,
                    dword(&data, at).unwrap() as usize,
                    READ_OVERFLOW - layout.bank_shift,
                )
                .unwrap()
            };
            assert_eq!(spans(3), [(1, 3), (5, 5), (7, 7), (9, 9)]);
            assert_eq!(spans(5), [(1, 1), (3, 3), (5, 5), (7, 7), (9, 9)]);
        }
    }

    #[test]
    fn legacy_checkpoint_preserves_text_at_the_end_of_the_record() {
        let snapshot = NativeSlot {
            script: "SCENE".into(),
            instruction: 390,
            resume: 326,
            entry: 30,
            speaker: "@sName".into(),
            line: "Dialogue".into(),
            assets: BTreeMap::from([(26, "SYSTEM\\WINDOW".into())]),
            preserve_view: true,
            ..NativeSlot::default()
        };
        let scene = LEGACY.classic.unwrap();
        let bytes = snapshot.encode_legacy(LEGACY, scene, SHIFT_JIS).unwrap();
        assert_eq!(bytes.len(), LEGACY.slot_size);
        let restored = NativeSlot::decode_legacy(&bytes, LEGACY, scene, SHIFT_JIS).unwrap();
        assert_eq!(restored.resume, snapshot.resume);
        assert_eq!(restored.assets, snapshot.assets);
        assert_eq!(restored.speaker, snapshot.speaker);
        assert_eq!(restored.line, snapshot.line);
        assert!(restored.preserve_view);
    }

    #[test]
    fn expanded_classic_save_writes_the_native_slot_layout_and_text_encoding() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("SAVEDATA");
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("system.dat"),
            vec![0; CLASSIC_EXPANDED.system_size()],
        )
        .unwrap();
        let mut saves = Saves::open(root.path(), 0x9f9a, encoding_rs::GBK).unwrap();
        saves.namespace = Some("TEST".into());
        let snapshot = NativeSlot {
            script: "SCENE".into(),
            instruction: 987,
            resume: 951,
            speaker: "说话者".into(),
            line: "保存当前场景。".into(),
            player_names: PlayerNames {
                given: "雨晴".into(),
                family: "林".into(),
            },
            preserve_view: true,
            layers: BTreeMap::from([(
                20,
                NativeLayer {
                    position: (512, -24),
                    extent: (720, 960),
                    fitted_extent: None,
                    pivot: (640, 360),
                    scale: 120,
                    visible: true,
                    order: 5,
                },
            )]),
            ..NativeSlot::default()
        };
        saves
            .write_slot(4, &snapshot, &RgbaImage::new(1, 1))
            .unwrap();
        let bytes = fs::read(directory.join("TEST04.dat")).unwrap();
        assert_eq!(bytes.len(), 72440);
        let encoded = encoding_rs::GBK.encode(&snapshot.line).0;
        assert_eq!(&bytes[0x117f8..0x117f8 + encoded.len()], encoded.as_ref());
        let loaded = saves.read_slot(4).unwrap().unwrap();
        assert_eq!(loaded.resume, snapshot.resume);
        assert_eq!(loaded.speaker, snapshot.speaker);
        assert_eq!(loaded.player_names.given, snapshot.player_names.given);
        assert_eq!(loaded.layers[&20].position, (512, -24));
        assert!(loaded.preserve_view);
    }

    #[test]
    fn write_slot_preserves_an_unrecognized_native_file_at_the_destination() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("AdvData");
        fs::create_dir(&data).unwrap();
        let mut saves = Saves::open(&data, 0x9f9a, SHIFT_JIS).unwrap();
        saves.namespace = Some("TEST".into());
        let mut snapshot = NativeSlot {
            script: "SCENE".into(),
            ..NativeSlot::default()
        };
        let thumbnail = RgbaImage::new(1, 1);
        saves.write_slot(0, &snapshot, &thumbnail).unwrap();
        let foreign = saves.directory.join("TEST01.DAT");
        fs::write(&foreign, b"a different save format").unwrap();

        let mut saves = Saves::open(&data, 0x9f9a, SHIFT_JIS).unwrap();
        assert!(!saves.exists(1));
        assert!(saves.write_slot(1, &snapshot, &thumbnail).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"a different save format");
        assert!(!saves.exists(1));

        snapshot.resume = 12;
        saves.write_slot(0, &snapshot, &thumbnail).unwrap();
        assert_eq!(saves.read_slot(0).unwrap().unwrap().resume, 12);
    }

    #[test]
    fn saves_fitted_background_with_native_scale_and_rgb_thumbnail() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("AdvData");
        fs::create_dir(&data).unwrap();
        let mut saves = Saves::open(&data, 0xabb6, SHIFT_JIS).unwrap();
        saves.namespace = Some("TEST".into());
        let snapshot = NativeSlot {
            script: "SCENE".into(),
            custom_names: true,
            layers: BTreeMap::from([(
                0,
                NativeLayer {
                    position: (-320, -180),
                    extent: (1920, 1080),
                    fitted_extent: Some((1280, 720)),
                    pivot: (0, 0),
                    scale: 100,
                    visible: true,
                    order: 0,
                },
            )]),
            ..NativeSlot::default()
        };
        let thumbnail = RgbaImage::from_pixel(32, 18, image::Rgba([10, 20, 30, 255]));
        saves.write_slot(4, &snapshot, &thumbnail).unwrap();
        let restored = saves.read_slot(4).unwrap().unwrap();
        assert!(restored.custom_names);
        assert_eq!(restored.layers[&0].fitted_extent, Some((1280, 720)));
        assert_eq!(restored.layers[&0].position, (-320, -180));
        let picture = image::open(data.join("SAVEDATA/TEST04.png")).unwrap();
        assert_eq!(picture.color(), image::ColorType::Rgb8);
        assert_eq!((picture.width(), picture.height()), (128, 72));
    }

    #[test]
    fn writing_an_original_setting_preserves_the_opaque_system_data() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("SAVEDATA");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("SYSTEM.DAT");
        let mut original = vec![0; MODERN.system_size()];
        original[MODERN.header + 18] = 0x80;
        fs::write(&path, &original).unwrap();

        let mut saves = Saves::open(root.path(), 0xabb6, SHIFT_JIS).unwrap();
        saves.update_global(99, 7, 57);
        saves.flush_globals().unwrap();

        original[60..64].copy_from_slice(&57i32.to_le_bytes());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(
            Saves::open(root.path(), 0xabb6, SHIFT_JIS)
                .unwrap()
                .global(99, 7),
            Some(57)
        );
    }
}
