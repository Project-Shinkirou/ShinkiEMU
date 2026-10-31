//! `sysparams.txt`: the original configuration table (tab-separated, CRLF,
//! sorted by key). Musica maps its configuration onto the keys both
//! executables read (perseus.exe sub_47FCC0 / sub_480430, eden_en.exe
//! sub_4B5C90 / sub_4B5570); every other key stays verbatim. Settings the
//! title's original has no key for go to `<game>/.shinki/musica/musica-settings.txt`
//! in the same layout.

use super::*;
use crate::{MusicaConfigState, MusicaPlayMode};

pub(super) const SYSPARAMS: &str = "sysparams.txt";
pub(super) const MUSICA_SETTINGS: &str = "musica-settings.txt";
const QUICK_SAVE: &[u8] = b"quickSaveFileNumber";
const AUTO_SAVE: &[u8] = b"autoSaveFileNumber";

/// perseus.exe voice table `off_518AC0`, in Musica's character order; the
/// original `etc` key is Musica's `mot` voice group. eden_en.exe has eight
/// unrelated voices (`off_595B50`), so its Musica toggles live in
/// `musica-settings.txt`.
fn original_voice_keys(title: MinoriTitle) -> Option<[&'static [u8]; 5]> {
    match title {
        MinoriTitle::Perseus => Some([b"ren", b"sui", b"aya", b"tou", b"etc"]),
        MinoriTitle::EdenEnglish => None,
    }
}

const MUSICA_VOICE_KEYS: [&[u8]; 5] = [
    b"voice_ren",
    b"voice_sui",
    b"voice_aya",
    b"voice_tou",
    b"voice_mot",
];

fn read_table(path: &Path) -> SessionResult<Option<Properties>> {
    read_optional(path)?
        .map(|bytes| minori_data::decode_settings(&bytes))
        .transpose()
        .map_err(core_error)
}

/// Skips the write when `path` already decodes to `table`.
fn write_table(path: &Path, table: &Properties) -> SessionResult<()> {
    if read_table(path).is_ok_and(|stored| stored.as_ref() == Some(table)) {
        return Ok(());
    }
    let bytes = minori_data::encode_settings(table).map_err(core_error)?;
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return Err(error(
            "SHINKI_EMU_MUSICA_SAVE_PATH",
            "settings path has no parent directory",
        ));
    };
    replace_file(dir, name, &bytes)
}

fn number(table: &Properties, key: &[u8]) -> Option<i64> {
    integer(table.get(key)?)
}

fn percent(table: &Properties, key: &[u8]) -> Option<u8> {
    number(table, key)
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| *value <= 100)
}

fn flag(table: &Properties, key: &[u8]) -> Option<bool> {
    number(table, key).map(|value| value != 0)
}

fn put(table: &mut Properties, key: &[u8], value: impl ToString) {
    table.insert(key.to_vec(), value.to_string().into_bytes());
}

fn put_flag(table: &mut Properties, key: &[u8], value: bool) {
    put(table, key, u8::from(value));
}

/// Overlays the stored keys onto `config`; absent or out-of-range values keep
/// the incoming setting, as the original keeps its default.
fn read_config(
    title: MinoriTitle,
    sysparams: &Properties,
    musica: &Properties,
    config: &mut MusicaConfigState,
) {
    let speeds = [
        (&b"messageSpeedTBR"[..], &mut config.message_speed_unread),
        (b"messageSpeedRead", &mut config.message_speed_read),
        (b"messageSpeedAutoPlay", &mut config.message_speed_auto_play),
        (b"volumeBGM", &mut config.bgm_volume),
        (b"volumeVoice", &mut config.voice_volume),
        (b"volumeSE", &mut config.se_volume),
    ];
    for (key, field) in speeds {
        if let Some(value) = percent(sysparams, key) {
            *field = value;
        }
    }
    if let Some(auto) = flag(sysparams, b"playMode") {
        config.preferred_play_mode = if auto {
            MusicaPlayMode::Auto
        } else {
            MusicaPlayMode::Skip
        };
    }
    // 0 windowed, 1 exclusive fullscreen, 2 borderless, 3 maximized
    // (eden_en.exe sub_4FDDD0).
    if let Some(mode) = number(sysparams, b"screenMode") {
        config.fullscreen = mode == 1 || mode == 2;
    }
    let flags = [
        (&b"screenEffect"[..], &mut config.screen_effect),
        (b"animation", &mut config.animation),
        (b"shadow", &mut config.text_shadow),
        (b"backlogVoicePlayback", &mut config.backlog_voice_playback),
        (b"stopVoiceAtNext", &mut config.stop_voice_at_next_message),
        (b"progressInBackground", &mut config.progress_in_background),
        (b"muteBGM", &mut config.bgm_muted),
        (b"muteVoice", &mut config.voice_muted),
        (b"muteSE", &mut config.se_muted),
    ];
    for (key, field) in flags {
        if let Some(value) = flag(sysparams, key) {
            *field = value;
        }
    }
    let (table, keys) = match original_voice_keys(title) {
        Some(keys) => (sysparams, keys),
        None => (musica, MUSICA_VOICE_KEYS),
    };
    for (field, key) in config.character_voice_enabled.iter_mut().zip(keys) {
        if let Some(value) = flag(table, key) {
            *field = value;
        }
    }
}

fn write_config(
    title: MinoriTitle,
    sysparams: &mut Properties,
    musica: &mut Properties,
    config: &MusicaConfigState,
) {
    put(sysparams, b"messageSpeedTBR", config.message_speed_unread);
    put(sysparams, b"messageSpeedRead", config.message_speed_read);
    put(
        sysparams,
        b"messageSpeedAutoPlay",
        config.message_speed_auto_play,
    );
    put_flag(
        sysparams,
        b"playMode",
        config.preferred_play_mode == MusicaPlayMode::Auto,
    );
    let stored = number(sysparams, b"screenMode");
    let fullscreen = matches!(stored, Some(1 | 2));
    if config.fullscreen != fullscreen || stored.is_none() {
        put(sysparams, b"screenMode", u8::from(config.fullscreen));
    }
    put_flag(sysparams, b"screenEffect", config.screen_effect);
    put_flag(sysparams, b"animation", config.animation);
    put_flag(sysparams, b"shadow", config.text_shadow);
    put_flag(
        sysparams,
        b"backlogVoicePlayback",
        config.backlog_voice_playback,
    );
    put_flag(
        sysparams,
        b"stopVoiceAtNext",
        config.stop_voice_at_next_message,
    );
    put_flag(
        sysparams,
        b"progressInBackground",
        config.progress_in_background,
    );
    put(sysparams, b"volumeBGM", config.bgm_volume);
    put(sysparams, b"volumeVoice", config.voice_volume);
    put(sysparams, b"volumeSE", config.se_volume);
    put_flag(sysparams, b"muteBGM", config.bgm_muted);
    put_flag(sysparams, b"muteVoice", config.voice_muted);
    put_flag(sysparams, b"muteSE", config.se_muted);
    let (table, keys) = match original_voice_keys(title) {
        Some(keys) => (sysparams, keys),
        None => (musica, MUSICA_VOICE_KEYS),
    };
    for (enabled, key) in config.character_voice_enabled.iter().zip(keys) {
        put_flag(table, key, *enabled);
    }
}

impl NativeDirectory {
    fn musica_settings_path(&self) -> PathBuf {
        self.game_root.join(".shinki/musica").join(MUSICA_SETTINGS)
    }

    pub(super) fn settings(&self) -> SessionResult<Option<Properties>> {
        read_table(&self.root.join(SYSPARAMS))
    }

    pub(super) fn musica_settings(&self) -> SessionResult<Properties> {
        read_table(&self.musica_settings_path()).map(Option::unwrap_or_default)
    }

    fn write_settings(&self, sysparams: &Properties, musica: &Properties) -> SessionResult<()> {
        write_table(&self.root.join(SYSPARAMS), sysparams)?;
        if !musica.is_empty() {
            write_table(&self.musica_settings_path(), musica)?;
        }
        Ok(())
    }
}

impl NativeFiles {
    pub(crate) fn auto_save_cycle(&self) -> u64 {
        self.settings
            .as_ref()
            .and_then(|table| number(table, b"autoSaveCycle"))
            .unwrap_or_else(|| match self.dir.as_ref().map(|dir| dir.title) {
                Some(MinoriTitle::EdenEnglish) => 200,
                Some(MinoriTitle::Perseus) => 400,
                None => 0,
            })
            .max(0) as u64
    }
    pub(crate) fn auto_save_backlog_limit(&self) -> usize {
        self.settings
            .as_ref()
            .and_then(|table| number(table, b"autoSaveBacklogLimit"))
            .unwrap_or(100)
            .max(0) as usize
    }
    /// The stored configuration over `fallback`; `fallback` itself while no
    /// `sysparams.txt` exists.
    pub(crate) fn configuration(&self, mut fallback: MusicaConfigState) -> MusicaConfigState {
        let Some(dir) = &self.dir else {
            return self.legacy_config.clone().unwrap_or(fallback);
        };
        if let Some(sysparams) = &self.settings {
            read_config(dir.title, sysparams, &self.musica, &mut fallback);
        }
        fallback
    }

    /// Rewrites the mapped keys; unmapped keys and an unchanged file stay as
    /// they are.
    pub(crate) fn store_configuration(&mut self, config: &MusicaConfigState) -> SessionResult<()> {
        let Some(dir) = &self.dir else {
            self.warn_unpersisted();
            return Ok(());
        };
        let mut sysparams = self.settings.clone().unwrap_or_default();
        let mut musica = self.musica.clone();
        write_config(dir.title, &mut sysparams, &mut musica, config);
        dir.write_settings(&sysparams, &musica)?;
        self.settings = Some(sysparams);
        self.musica = musica;
        Ok(())
    }

    fn cursor(&self, key: &[u8]) -> u32 {
        self.settings
            .as_ref()
            .and_then(|sysparams| number(sysparams, key))
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value < crate::storage::SAVE_PAGE_WIDTH)
            .unwrap_or(0)
    }

    fn store_cursor(&mut self, key: &[u8], next: u32) -> SessionResult<()> {
        let Some(dir) = &self.dir else {
            self.warn_unpersisted();
            return Ok(());
        };
        let mut sysparams = self.settings.clone().unwrap_or_default();
        put(&mut sysparams, key, next);
        write_table(&dir.root.join(SYSPARAMS), &sysparams)?;
        self.settings = Some(sysparams);
        Ok(())
    }

    /// `quickSaveFileNumber` verbatim; zero when absent or outside `0..=9`.
    pub(crate) fn quick_cursor(&self) -> u32 {
        if self.dir.is_none() {
            return self.legacy_quick_cursor.unwrap_or(0);
        }
        self.cursor(QUICK_SAVE)
    }

    pub(crate) fn store_quick_cursor(&mut self, next: u32) -> SessionResult<()> {
        self.store_cursor(QUICK_SAVE, next)
    }

    /// `autoSaveFileNumber` verbatim; zero when absent or outside `0..=9`.
    pub(crate) fn auto_cursor(&self) -> u32 {
        self.cursor(AUTO_SAVE)
    }

    pub(crate) fn store_auto_cursor(&mut self, next: u32) -> SessionResult<()> {
        self.store_cursor(AUTO_SAVE, next)
    }

    /// One-time, no-clobber import of `configuration.json` and
    /// `quick-cursor.json` from `<game>/.shinki/musica/saves/` into
    /// `sysparams.txt`; each imported source becomes `<name>.imported`. An
    /// existing `sysparams.txt` is never overwritten and the JSON stays put.
    pub(crate) fn import_legacy_settings(
        &mut self,
        game_root: &Path,
        game: Hash256,
    ) -> SessionResult<()> {
        if self.dir.is_some() && self.settings.is_some() {
            return Ok(());
        }
        let saves = game_root.join(".shinki/musica/saves");
        let config = legacy::configuration(&saves, game)?;
        let cursor = legacy::quick_cursor(&saves, game)?;
        let Some(dir) = &self.dir else {
            self.legacy_config = config;
            self.legacy_quick_cursor = cursor;
            return Ok(());
        };
        if config.is_none() && cursor.is_none() {
            return Ok(());
        }
        let mut sysparams = Properties::new();
        let mut musica = self.musica.clone();
        if let Some(config) = &config {
            write_config(dir.title, &mut sysparams, &mut musica, config);
        }
        if let Some(cursor) = cursor {
            put(&mut sysparams, QUICK_SAVE, cursor);
        }
        dir.write_settings(&sysparams, &musica)?;
        self.settings = Some(sysparams);
        self.musica = musica;
        if config.is_some() {
            legacy::mark_imported(&saves, legacy::CONFIGURATION)?;
        }
        if cursor.is_some() {
            legacy::mark_imported(&saves, legacy::QUICK_CURSOR)?;
        }
        Ok(())
    }
}
