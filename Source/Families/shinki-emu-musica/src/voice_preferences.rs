use shinki_emu_sdk::runtime::*;
use std::{borrow::Cow, collections::BTreeSet};
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct VoiceCharacter {
    pub prefix: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}
fn enabled_default() -> bool {
    true
}
#[derive(Clone, Debug)]
pub(crate) struct VoicePreferences<'a> {
    pub backlog_voice_playback: bool,
    pub characters: Cow<'a, [VoiceCharacter]>,
}
impl Default for VoicePreferences<'_> {
    fn default() -> Self {
        Self {
            backlog_voice_playback: true,
            characters: Cow::Borrowed(&[]),
        }
    }
}
fn prefix(name: &str) -> &str {
    let stem = name
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .split('.')
        .next()
        .unwrap_or(name);
    stem.split(['-', '_'])
        .next()
        .unwrap_or(stem)
        .trim_end_matches(|c: char| c.is_ascii_digit())
}
impl VoicePreferences<'static> {
    pub(crate) fn fields() -> Vec<crate::provider::MusicaField> {
        vec![
            crate::provider::MusicaField::new(
                "backlog_voice_playback",
                "回想中播放语音",
                "初始语音设置",
                ConfigKind::Bool,
                ConfigValue::Bool(true),
                "是否允许在回想记录中重播角色语音。",
            ),
            crate::provider::MusicaField::new(
                "voice_characters",
                "角色语音列表",
                "初始语音设置",
                ConfigKind::String { max_bytes: 4096 },
                ConfigValue::String("[]".into()),
                "JSON 数组：prefix、label、enabled；实际声档前缀优先发现，发行版配置补充名称、顺序和初始开关。",
            ),
        ]
    }
    pub fn resolve(
        entries: &[ConfigEntry],
        archive: &crate::MusicaMountedVfs,
    ) -> SessionResult<Self> {
        let backlog = entries
            .iter()
            .find(|entry| entry.id == "backlog_voice_playback")
            .map(|entry| &entry.value);
        let json = entries
            .iter()
            .find(|entry| entry.id == "voice_characters")
            .map(|entry| &entry.value);
        let (Some(ConfigValue::Bool(backlog)), Some(ConfigValue::String(json))) = (backlog, json)
        else {
            return Err(SessionError::invalid(
                "SHINKI_EMU_MUSICA_CONFIG",
                "voice preferences are missing or invalid",
            ));
        };
        let mut characters: Vec<VoiceCharacter> = serde_json::from_str(json).map_err(|_| {
            SessionError::invalid("SHINKI_EMU_MUSICA_CONFIG", "invalid voice characters JSON")
        })?;
        let mut seen = BTreeSet::new();
        for character in &mut characters {
            if character.prefix.is_empty()
                || character.prefix.len() > 64
                || !character.prefix.bytes().all(|b| b.is_ascii_alphanumeric())
                || !seen.insert(character.prefix.clone())
                || character.label.len() > 256
            {
                return Err(SessionError::invalid(
                    "SHINKI_EMU_MUSICA_CONFIG",
                    "invalid or duplicated voice character",
                ));
            }
            if character.label.is_empty() {
                character.label = character.prefix.clone();
            }
        }
        for entry in &archive.manifest().entries {
            if let Some(name) = entry.uri.strip_prefix("musica:/voice/")
                && entry
                    .uri
                    .rsplit_once('.')
                    .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("ogg"))
            {
                let prefix = prefix(name);
                if !prefix.is_empty() && seen.insert(prefix.into()) {
                    characters.push(VoiceCharacter {
                        prefix: prefix.into(),
                        label: prefix.into(),
                        enabled: true,
                    });
                }
            }
        }
        if characters.len() > 64 {
            return Err(SessionError::invalid(
                "SHINKI_EMU_MUSICA_CONFIG",
                "voice character count exceeds 64",
            ));
        }
        Ok(Self {
            backlog_voice_playback: *backlog,
            characters: Cow::Owned(characters),
        })
    }
}
impl VoicePreferences<'_> {
    pub fn enabled(&self, uri: &str) -> bool {
        let Some(name) = uri.strip_prefix("musica:/voice/") else {
            return true;
        };
        self.characters
            .iter()
            .find(|character| character.prefix == prefix(name))
            .is_none_or(|character| character.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovered_characters_and_declared_order_drive_voice_and_config_controls() {
        let root = tempfile::tempdir().unwrap();
        crate::test_fixture::game(root.path(), b".end\r\n");
        crate::test_fixture::assets(
            root.path(),
            "voice",
            &[
                ("visitor-001.ogg", b"directory-fixture"),
                ("guest_003.ogg", b"directory-fixture"),
                ("actor007.ogg", b"directory-fixture"),
            ],
        );
        let archive = crate::mount_musica(root.path(), std::path::Path::new("key.toml")).unwrap();
        let entries = resolve_config(
            &crate::musica_descriptor().configuration,
            &[ConfigEntry {
                id: "voice_characters".into(),
                value: ConfigValue::String(
                    r#"[{"prefix":"visitor","label":"Visitor","enabled":false}]"#.into(),
                ),
            }],
        )
        .unwrap();
        let preferences = VoicePreferences::resolve(&entries, &archive).unwrap();
        assert_eq!(preferences.characters.len(), 3);
        assert_eq!(preferences.characters[0].prefix, "visitor");
        assert!(!preferences.enabled("musica:/voice/visitor-002.ogg"));
        assert!(preferences.enabled("musica:/voice/guest_003.ogg"));
        assert!(preferences.enabled("musica:/voice/actor008.ogg"));
        assert!(preferences.enabled("musica:/voice/unlisted-001.ogg"));
        let mut config = crate::MusicaConfigState {
            character_voices: preferences.characters.into_owned(),
            ..Default::default()
        };
        config
            .edit(crate::MusicaConfigControl::ToggleCharacterVoice(2))
            .unwrap();
        assert!(
            !config
                .voice_preferences()
                .enabled("musica:/voice/guest_003.ogg")
        );
        assert!(
            config
                .edit(crate::MusicaConfigControl::ToggleCharacterVoice(3))
                .is_err()
        );
    }
}
