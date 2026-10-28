use super::*;

#[test]
fn caption_charset_reads_native_initial_config_default_and_boolean() {
    let root = tempfile::tempdir().unwrap();
    assert!(font_shift_jis(root.path()).unwrap());
    for (value, expected) in [(0, false), (1, true), (-1, true), (2, true)] {
        std::fs::write(
            root.path().join("initial.cfg"),
            format!("[CMVS_INIT_CFG]\nFONT_SHIFT_JIS={value}\n"),
        )
        .unwrap();
        assert_eq!(font_shift_jis(root.path()).unwrap(), expected);
    }
}

#[derive(Default)]
struct Values {
    mode: Option<u32>,
    font: Option<String>,
}
impl Registry for Values {
    fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError> {
        assert_eq!((key, value), ("Fixture", "ConfigPut"));
        Ok(self.mode)
    }
    fn string(&self, key: &str, value: &str) -> Result<Option<String>, CoreError> {
        assert_eq!((key, value), ("Fixture\\param", "MI_Font"));
        Ok(self.font.clone())
    }
}

fn initial(root: &Path, mode: i32, save: u32) {
    std::fs::write(
        root.join("initial.cfg"),
        format!("[CMVS_INIT_CFG]\nREG=Fixture\nCONFIG_PUT={mode}\nSAVE_FOLDER={save}\n"),
    )
    .unwrap();
}

#[test]
fn font_flags_read_native_defaults_ini_and_registry_parameters() {
    struct Flags;
    impl Registry for Flags {
        fn number(&self, key: &str, name: &str) -> Result<Option<u32>, CoreError> {
            assert_eq!(key, "Fixture\\param");
            Ok(match name {
                "MI_TextBold" => Some(0),
                "MI_TextRevisionAlpha" => Some(2),
                _ => panic!("unexpected registry field"),
            })
        }
        fn string(&self, _: &str, _: &str) -> Result<Option<String>, CoreError> {
            panic!("font flags do not query strings")
        }
    }
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve_font_flags(root.path(), &Values::default()).unwrap(),
        [false, false]
    );
    std::fs::write(
        root.path().join("cmvs.cfg"),
        b"[MESSAGE_INFO]\nTEXT_BOLD=-1\nTEXT_REVISION_ALPHA=0\n",
    )
    .unwrap();
    assert_eq!(
        resolve_font_flags(root.path(), &Values::default()).unwrap(),
        [true, false]
    );
    initial(root.path(), 1, 0);
    assert_eq!(
        resolve_font_flags(root.path(), &Flags).unwrap(),
        [false, true]
    );
}

#[test]
fn config_defaults_and_registry_override_follow_native_selection() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        DEFAULT_FONT
    );
    std::fs::write(
        root.path().join("cmvs.cfg"),
        b"[MESSAGE_INFO]\nFONT=\"Local Font\"\n",
    )
    .unwrap();
    for mode in [0, 2, 4, -4] {
        initial(root.path(), mode, 0);
        assert_eq!(
            resolve_font(
                root.path(),
                &Values {
                    mode: None,
                    font: Some("Registry Font".into())
                }
            )
            .unwrap(),
            "Local Font"
        );
    }
    initial(root.path(), 1, 0);
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        "Local Font"
    );
    assert_eq!(
        resolve_font(
            root.path(),
            &Values {
                mode: None,
                font: Some("Registry Font".into())
            }
        )
        .unwrap(),
        "Registry Font"
    );
    assert_eq!(
        resolve_font(
            root.path(),
            &Values {
                mode: None,
                font: Some(String::new())
            }
        )
        .unwrap(),
        DEFAULT_FONT
    );
    initial(root.path(), 3, 0);
    assert_eq!(
        resolve_font(
            root.path(),
            &Values {
                mode: Some(1),
                font: Some("Selected Font".into())
            }
        )
        .unwrap(),
        "Selected Font"
    );
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        "Local Font"
    );
}

#[test]
fn cp932_font_name_and_utf16_ini_are_decoded_without_substitution() {
    let root = tempfile::tempdir().unwrap();
    let text = format!("[message_info]\nfont={DEFAULT_FONT}\n");
    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
    assert!(!invalid);
    std::fs::write(root.path().join("cmvs.cfg"), &bytes).unwrap();
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        DEFAULT_FONT
    );
    let bytes = [0xff, 0xfe]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
    std::fs::write(root.path().join("cmvs.cfg"), bytes).unwrap();
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        DEFAULT_FONT
    );
}

#[test]
fn external_config_root_and_invalid_values_do_not_fall_back() {
    let root = tempfile::tempdir().unwrap();
    initial(root.path(), 2, 1);
    assert_eq!(
        resolve_font(root.path(), &Values::default())
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_CONFIG_ROOT_UNBOUND"
    );
    initial(root.path(), 0, 0);
    std::fs::write(root.path().join("cmvs.cfg"), b"[MESSAGE_INFO]\nFONT=\x81").unwrap();
    assert!(resolve_font(root.path(), &Values::default()).is_err());
    std::fs::write(
        root.path().join("cmvs.cfg"),
        format!("[MESSAGE_INFO]\nFONT={}\n", "x".repeat(64)),
    )
    .unwrap();
    assert!(resolve_font(root.path(), &Values::default()).is_err());
}

#[test]
fn native_stage_uses_window_mode_and_custom_dimensions_not_fullscreen_or_host_raster() {
    let root = tempfile::tempdir().unwrap();
    for (mode, expected) in [(0, [640, 480]), (2, [1024, 768]), (9, [1920, 1080])] {
        std::fs::write(root.path().join("cmvs.cfg"), format!(
            "[CMVS_SYSTEM_MAIN]\nWINDOW_SIZE={mode}\n[RESOLVING_POWER_INFO]\nWINDOW_WIDTH=1920\nWINDOW_HEIGHT=1080\nFULL_SCREEN_WIDTH=2560\nFULL_SCREEN_HEIGHT=1440\n"
        )).unwrap();
        assert_eq!(
            resolve_display_sizes(root.path(), &Values::default()).unwrap(),
            (expected, expected)
        );
    }
    std::fs::write(root.path().join("cmvs.cfg"),
        "[CMVS_SYSTEM_MAIN]\nWINDOW_SIZE=9\n[RESOLVING_POWER_INFO]\nWINDOW_WIDTH=0\nWINDOW_HEIGHT=1080\n").unwrap();
    assert!(resolve_display_sizes(root.path(), &Values::default()).is_err());
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[WINDOW_RESOLVING]\nSELECT=1\n",
    )
    .unwrap();
    assert_eq!(
        resolve_display_sizes(root.path(), &Values::default())
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_DISPLAY_MODE_UNBOUND"
    );
}

#[test]
fn native_stage_registry_overrides_only_selected_values_in_param_namespace() {
    struct Dimensions;
    impl Registry for Dimensions {
        fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError> {
            assert_eq!(key, "Fixture\\param");
            Ok(match value {
                "CS_WindowSize" => Some(9),
                "RP_WindowWidth" => Some(1600),
                _ => None,
            })
        }
        fn string(&self, _: &str, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
    }
    let root = tempfile::tempdir().unwrap();
    initial(root.path(), 1, 0);
    std::fs::write(root.path().join("cmvs.cfg"),
        "[CMVS_SYSTEM_MAIN]\nWINDOW_SIZE=0\n[RESOLVING_POWER_INFO]\nWINDOW_WIDTH=1920\nWINDOW_HEIGHT=900\n").unwrap();
    assert_eq!(
        resolve_display_sizes(root.path(), &Dimensions).unwrap(),
        ([1600, 900], [1600, 900])
    );
    initial(root.path(), 0, 0);
    assert_eq!(
        resolve_display_sizes(root.path(), &Dimensions).unwrap(),
        ([640, 480], [640, 480])
    );
}

#[test]
fn saved_resolution_separates_base_coordinates_from_selected_render_mode() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("cmvs.cfg"),
        "[WINDOW_RESOLVING]\nSELECT=1\nUSE=2,5\nBASE=1920,1080\nMODE02=1280,720,1280,768,1280,800\n[CMVS_SYSTEM_MAIN]\nWINDOW_SIZE=0\n").unwrap();
    assert_eq!(
        resolve_display_sizes(root.path(), &Values::default()).unwrap(),
        ([1920, 1080], [1280, 720])
    );
}

#[test]
fn window_mode_uses_native_registry_precedence_and_nonzero_windowed_values() {
    struct WindowMode(Option<u32>);
    impl Registry for WindowMode {
        fn number(&self, key: &str, name: &str) -> Result<Option<u32>, CoreError> {
            assert_eq!((key, name), ("Fixture\\param", "WS_WindowMode"));
            Ok(self.0)
        }
        fn string(&self, _: &str, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
    }
    let root = tempfile::tempdir().unwrap();
    assert!(!resolve_windowed(root.path(), &WindowMode(None)).unwrap());
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[WINDOW_STATUS]\nWINDOW_MODE=1\n",
    )
    .unwrap();
    initial(root.path(), 1, 0);
    assert!(resolve_windowed(root.path(), &WindowMode(None)).unwrap());
    assert!(!resolve_windowed(root.path(), &WindowMode(Some(0))).unwrap());
    initial(root.path(), 0, 0);
    assert!(resolve_windowed(root.path(), &WindowMode(Some(0))).unwrap());
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[WINDOW_STATUS]\nWINDOW_MODE=0\n",
    )
    .unwrap();
    initial(root.path(), 1, 0);
    assert!(resolve_windowed(root.path(), &WindowMode(Some(2))).unwrap());
}

#[test]
fn native_view_policy_registry_values_override_ini_and_out_of_range_modes_reset() {
    struct Policies;
    impl Registry for Policies {
        fn number(&self, key: &str, name: &str) -> Result<Option<u32>, CoreError> {
            assert_eq!(key, "Fixture\\param");
            Ok(Some(match name {
                "DC_AspectRatioScreen" => 2,
                "DC_AspectRatioPoint" => 4,
                _ => panic!("unexpected native view policy key"),
            }))
        }
        fn string(&self, _: &str, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
    }
    let root = tempfile::tempdir().unwrap();
    assert_eq!(resolve_view_policy(root.path(), &Policies).unwrap(), [1, 2]);
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[DISPLAY_CONFIG]\nASPECT_RATIO_SCREEN=3\nASPECT_RATIO_POINT=0\n",
    )
    .unwrap();
    initial(root.path(), 1, 0);
    assert_eq!(resolve_view_policy(root.path(), &Policies).unwrap(), [2, 4]);
    initial(root.path(), 0, 0);
    assert_eq!(resolve_view_policy(root.path(), &Policies).unwrap(), [3, 0]);
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[DISPLAY_CONFIG]\nASPECT_RATIO_SCREEN=-1\nASPECT_RATIO_POINT=5\n",
    )
    .unwrap();
    assert_eq!(resolve_view_policy(root.path(), &Policies).unwrap(), [1, 2]);
}

#[test]
fn legacy_native_fullscreen_dimensions_are_distinct_and_registry_overrides_each_axis() {
    struct Fullscreen;
    impl Registry for Fullscreen {
        fn number(&self, key: &str, name: &str) -> Result<Option<u32>, CoreError> {
            assert_eq!(key, "Fixture\\param");
            Ok(match name {
                "RP_FullScreenWidth" => Some(1440),
                _ => None,
            })
        }
        fn string(&self, _: &str, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("cmvs.cfg"),
        "[CMVS_SYSTEM_MAIN]\nWINDOW_SIZE=9\n[RESOLVING_POWER_INFO]\nWINDOW_WIDTH=800\nWINDOW_HEIGHT=600\nFULL_SCREEN_WIDTH=1280\nFULL_SCREEN_HEIGHT=900\n").unwrap();
    assert_eq!(
        resolve_display_sizes(root.path(), &Fullscreen).unwrap(),
        ([800, 600], [800, 600])
    );
    assert_eq!(
        resolve_fullscreen_size(root.path(), &Fullscreen).unwrap(),
        Some([1280, 900])
    );
    initial(root.path(), 1, 0);
    assert_eq!(
        resolve_fullscreen_size(root.path(), &Fullscreen).unwrap(),
        Some([1440, 900])
    );
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[CMVS_SYSTEM_MAIN]\nWINDOW_SIZE=9\n[RESOLVING_POWER_INFO]\nFULL_SCREEN_HEIGHT=0\n",
    )
    .unwrap();
    assert_eq!(
        resolve_fullscreen_size(root.path(), &Fullscreen)
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_NATIVE_CONFIG"
    );
}

#[test]
fn saved_native_mode_does_not_fabricate_adapter_fullscreen_dimensions() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("cmvs.cfg"),
        "[WINDOW_RESOLVING]\nSELECT=1\nUSE=2,5\nBASE=1920,1080\nMODE02=1280,720,1280,768,1280,800\n").unwrap();
    assert_eq!(
        resolve_fullscreen_size(root.path(), &Values::default()).unwrap(),
        None
    );
}

#[test]
fn native_sound_format_uses_real_defaults_and_unsigned_buffer_clamp() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve_audio_configuration(root.path(), &Values::default()).unwrap(),
        ([22050, 3072], [1, 1])
    );
    for (quality, buffer, expected) in [
        (1, 0, [44100, 6144]),
        (1, 1, [44100, 8192]),
        (1, 2, [44100, 12288]),
        (1, 3, [44100, 16384]),
        (0, 2, [22050, 6144]),
        (-1, -1, [44100, 16384]),
        (0, 99, [22050, 8192]),
    ] {
        std::fs::write(
            root.path().join("cmvs.cfg"),
            format!("[SOUND_INFO]\nSOUND_QUALITY={quality}\nSOUND_BUFFER={buffer}\n"),
        )
        .unwrap();
        assert_eq!(
            resolve_audio_configuration(root.path(), &Values::default())
                .unwrap()
                .0,
            expected
        );
    }
}

#[test]
fn native_sound_format_consumes_selected_registry_parameters_before_ini() {
    struct SoundRegistry;
    impl Registry for SoundRegistry {
        fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError> {
            match (key, value) {
                ("Fixture\\param", "SO_SoundQuality") => Ok(Some(0)),
                ("Fixture\\param", "SO_SoundBuffer") => Ok(Some(2)),
                _ => Ok(None),
            }
        }
        fn string(&self, _: &str, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
    }
    let root = tempfile::tempdir().unwrap();
    initial(root.path(), 1, 0);
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[SOUND_INFO]\nSOUND_QUALITY=1\nSOUND_BUFFER=1\n",
    )
    .unwrap();
    assert_eq!(
        resolve_audio_configuration(root.path(), &SoundRegistry)
            .unwrap()
            .0,
        [22050, 6144]
    );
}

#[test]
fn configured_audio_gates_follow_se_then_voice_startup_without_invented_defaults() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[SOUND_INFO]\nSE_USE=0\nVOICE_USE=-1\n",
    )
    .unwrap();
    assert_eq!(
        resolve_audio_configuration(root.path(), &Values::default())
            .unwrap()
            .1,
        [1, 0]
    );
}

#[test]
fn portable_registry_store_keeps_registry_keys_types_and_absence() {
    let root = tempfile::tempdir().unwrap();
    initial(root.path(), 3, 0);
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[MESSAGE_INFO]\nFONT=Ini Font\nTEXT_BOLD=0\n",
    )
    .unwrap();
    let registry = registry::PortableRegistry::new(root.path());
    // No store: CONFIG_PUT=3 finds no ConfigPut and falls back to the INI.
    assert_eq!(resolve_font(root.path(), &registry).unwrap(), "Ini Font");
    std::fs::create_dir_all(root.path().join(".shinki/cmvs")).unwrap();
    let store = root.path().join(registry::REGISTRY_FILE);
    std::fs::write(
        &store,
        r#"{"fixture": {"configput": 1},
            "Fixture\\param\\": {"MI_Font": "Stored Font", "MI_TextBold": 1}}"#,
    )
    .unwrap();
    assert_eq!(resolve_font(root.path(), &registry).unwrap(), "Stored Font");
    assert_eq!(
        resolve_font_flags(root.path(), &registry).unwrap(),
        [true, false]
    );
    // A value of the other registry type fails like RegGetValueW.
    std::fs::write(
        &store,
        r#"{"Fixture": {"ConfigPut": 1}, "Fixture\\param": {"MI_Font": 7}}"#,
    )
    .unwrap();
    assert!(resolve_font(root.path(), &registry).is_err());
    std::fs::write(&store, "not json").unwrap();
    assert!(resolve_font(root.path(), &registry).is_err());
}
