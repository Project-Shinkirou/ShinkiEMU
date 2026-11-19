//! Native tab-separated settings, preserving every unmapped property.
//! Frontend settings without a configured native key stay in musica-settings.txt.

use super::*;
use crate::{MusicaConfigState, MusicaPlayMode};

pub(super) const SYSPARAMS: &str = "sysparams.txt";
pub(super) const MUSICA_SETTINGS: &str = "musica-settings.txt";
const QUICK_SAVE: &[u8] = b"quickSaveFileNumber";
const AUTO_SAVE: &[u8] = b"autoSaveFileNumber";

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
    identity: &SaveIdentity,
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
    for (index, character) in config.character_voices.iter_mut().enumerate() {
        let local_key = format!("voice_{}", character.prefix);
        let (table, key) = identity
            .voice_keys
            .as_ref()
            .and_then(|keys| keys.get(index))
            .map_or((musica, local_key.as_bytes()), |key| {
                (sysparams, key.as_bytes())
            });
        if let Some(value) = flag(table, key) {
            character.enabled = value;
        }
    }
}

fn write_config(
    identity: &SaveIdentity,
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
    for (index, character) in config.character_voices.iter().enumerate() {
        if let Some(key) = identity
            .voice_keys
            .as_ref()
            .and_then(|keys| keys.get(index))
        {
            put_flag(sysparams, key.as_bytes(), character.enabled);
        } else {
            put_flag(
                musica,
                format!("voice_{}", character.prefix).as_bytes(),
                character.enabled,
            );
        }
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
            .unwrap_or(self.dir.identity.auto_save_cycle.min(i64::MAX as u64) as i64)
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
        if let Some(sysparams) = &self.settings {
            read_config(&self.dir.identity, sysparams, &self.musica, &mut fallback);
        }
        fallback
    }

    /// Rewrites the mapped keys; unmapped keys and an unchanged file stay as
    /// they are.
    pub(crate) fn store_configuration(&mut self, config: &MusicaConfigState) -> SessionResult<()> {
        let dir = &self.dir;
        let mut sysparams = self.settings.clone().unwrap_or_default();
        let mut musica = self.musica.clone();
        write_config(&dir.identity, &mut sysparams, &mut musica, config);
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
        let dir = &self.dir;
        let mut sysparams = self.settings.clone().unwrap_or_default();
        put(&mut sysparams, key, next);
        write_table(&dir.root.join(SYSPARAMS), &sysparams)?;
        self.settings = Some(sysparams);
        Ok(())
    }

    /// `quickSaveFileNumber` verbatim; zero when absent or outside `0..=9`.
    pub(crate) fn quick_cursor(&self) -> u32 {
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
        if self.settings.is_some() {
            return Ok(());
        }
        let saves = game_root.join(".shinki/musica/saves");
        let config = legacy::configuration(&saves, game)?;
        let cursor = legacy::quick_cursor(&saves, game)?;
        if config.is_none() && cursor.is_none() {
            return Ok(());
        }
        let mut sysparams = Properties::new();
        let mut musica = self.musica.clone();
        if let Some(config) = &config {
            write_config(&self.dir.identity, &mut sysparams, &mut musica, config);
        }
        if let Some(cursor) = cursor {
            put(&mut sysparams, QUICK_SAVE, cursor);
        }
        self.dir.write_settings(&sysparams, &musica)?;
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

#[cfg(test)]
mod frontend_tests {
    use super::*;
    #[test]
    fn declared_voice_order_encodes_identical_legacy_native_bytes() {
        for extended in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let identity = crate::native_directory::tests::identity_fixture(root.path(), extended);
            let mut config = MusicaConfigState::default();
            let mut old_sysparams = Properties::new();
            let mut old_musica = Properties::new();
            write_config(&identity, &mut old_sysparams, &mut old_musica, &config);
            let values = [true, false, true, true, false];
            let prefixes = ["ren", "sui", "aya", "tou", "mot"];
            // Oracle: the pre-migration five-position native mapping.
            let keys = if extended {
                ["ren", "sui", "aya", "tou", "etc"]
            } else {
                [
                    "voice_ren",
                    "voice_sui",
                    "voice_aya",
                    "voice_tou",
                    "voice_mot",
                ]
            };
            let table = if extended {
                &mut old_sysparams
            } else {
                &mut old_musica
            };
            for (key, enabled) in keys.into_iter().zip(values) {
                put_flag(table, key.as_bytes(), enabled);
            }
            config.character_voices = prefixes
                .into_iter()
                .zip(values)
                .map(
                    |(prefix, enabled)| crate::voice_preferences::VoiceCharacter {
                        prefix: prefix.into(),
                        label: prefix.into(),
                        enabled,
                    },
                )
                .collect();
            let mut sysparams = Properties::new();
            let mut musica = Properties::new();
            write_config(&identity, &mut sysparams, &mut musica, &config);
            assert_eq!(
                minori_data::encode_settings(&sysparams).unwrap(),
                minori_data::encode_settings(&old_sysparams).unwrap()
            );
            assert_eq!(
                minori_data::encode_settings(&musica).unwrap(),
                minori_data::encode_settings(&old_musica).unwrap()
            );
            for character in &mut config.character_voices {
                character.enabled = !character.enabled;
            }
            read_config(&identity, &sysparams, &musica, &mut config);
            assert_eq!(
                config
                    .character_voices
                    .iter()
                    .map(|character| character.enabled)
                    .collect::<Vec<_>>(),
                values
            );
        }
    }
}
