use super::*;
use configparser::ini::Ini;
use std::path::Path;
mod registry;

pub(crate) const DEFAULT_FONT: &str = "ＭＳ ゴシック";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

fn failure() -> CoreError {
    invalid(
        "SHINKI_EMU_CMVS_NATIVE_CONFIG",
        "native configuration is invalid or unavailable",
    )
}

impl CmvsArchive {
    pub(crate) fn initialize_native_caption(
        &self,
        vm: &mut crate::CmvsPs2aVmState,
    ) -> Result<(), CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        vm.process_global_words.insert(
            crate::vm::NATIVE_FONT_SHIFT_JIS,
            u32::from(font_shift_jis(root)?),
        );
        let registry = registry::PortableRegistry::new(root);
        let [bold, alternate] = resolve_font_flags(root, &registry)?;
        vm.write_interpreter_word(1476, u32::from(bold));
        vm.write_interpreter_word(1496, u32::from(alternate));
        vm.native_default_font = resolve_font(root, &registry)?;
        Ok(())
    }

    /// 41D7F0 configures D08 slot zero before loading start.ps3. Script
    /// commands may subsequently replace that slot or add later search slots.
    pub(crate) fn initialize_native_script_search(
        &self,
        vm: &mut crate::CmvsPs2aVmState,
    ) -> Result<(), CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        let path = string(
            &read_ini(root, "cmvs.cfg")?,
            "CMVS_SYSTEM_MAIN",
            "SCRIPT_INIT_PATH",
        )
        .unwrap_or_default();
        let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(&path);
        if malformed || bytes.len() >= 256 {
            return Err(failure());
        }
        native::validate_search_path(&path)?;
        vm.resource_search_paths
            .entry(crate::CmvsResourceSearchBank::Script)
            .or_default()
            .insert(0, path);
        Ok(())
    }

    pub(crate) fn native_default_font(&self) -> Result<String, CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        resolve_font(root, &registry::PortableRegistry::new(root))
    }

    pub(crate) fn native_display_sizes(&self) -> Result<([u32; 2], [u32; 2]), CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        resolve_display_sizes(root, &registry::PortableRegistry::new(root))
    }

    pub(crate) fn native_windowed(&self) -> Result<bool, CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        resolve_windowed(root, &registry::PortableRegistry::new(root))
    }

    pub(crate) fn native_audio_configuration(&self) -> Result<([u32; 2], [u32; 2]), CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        resolve_audio_configuration(root, &registry::PortableRegistry::new(root))
    }

    pub(crate) fn initialize_native_view_policy(
        &self,
        vm: &mut crate::CmvsPs2aVmState,
    ) -> Result<(), CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        let registry = registry::PortableRegistry::new(root);
        let [screen, point] = resolve_view_policy(root, &registry)?;
        vm.component_words.insert((1920_u32 << 16) | 100, screen);
        vm.component_words.insert((1920_u32 << 16) | 104, point);
        vm.native_view.fullscreen_size = resolve_fullscreen_size(root, &registry)?;
        Ok(())
    }
}

// 41D7F0 reads the saved registry parameter first, then the INI default. The
// registry values come from the portable `.shinki/cmvs/registry.json` store.
fn resolve_font_flags(root: &Path, registry: &impl Registry) -> Result<[bool; 2], CoreError> {
    let (in_registry, key) = configuration_source(root, registry)?;
    let ini = read_ini(root, "cmvs.cfg")?;
    let parameter_key = in_registry.then(|| format!("{}\\param", key.trim_end_matches('\\')));
    let number = |name: &str, registry_name: &str| -> Result<bool, CoreError> {
        if let Some(key) = parameter_key.as_deref()
            && let Some(value) = registry.number(key, registry_name)?
        {
            return Ok(value != 0);
        }
        Ok(integer(&ini, "MESSAGE_INFO", name, 0)? != 0)
    };
    Ok([
        number("TEXT_BOLD", "MI_TextBold")?,
        number("TEXT_REVISION_ALPHA", "MI_TextRevisionAlpha")?,
    ])
}

fn font_shift_jis(root: &Path) -> Result<bool, CoreError> {
    Ok(integer(
        &read_ini(root, "initial.cfg")?,
        "CMVS_INIT_CFG",
        "FONT_SHIFT_JIS",
        1,
    )? != 0)
}

trait Registry {
    fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError>;
    fn string(&self, key: &str, value: &str) -> Result<Option<String>, CoreError>;
}

fn configuration_source(
    root: &Path,
    registry: &impl Registry,
) -> Result<(bool, String), CoreError> {
    let initial = read_ini(root, "initial.cfg")?;
    let key = string(&initial, "CMVS_INIT_CFG", "REG").unwrap_or_default();
    let requested = integer(&initial, "CMVS_INIT_CFG", "CONFIG_PUT", 0)?.unsigned_abs() % 4;
    let (mode, in_registry) = if requested == 3 {
        match registry.number(&key, "ConfigPut")? {
            Some(value) => (value & 3, value == 1),
            None => (0, false),
        }
    } else {
        (requested, requested == 1)
    };
    let save_root = integer(&initial, "CMVS_INIT_CFG", "SAVE_FOLDER", 0)? as u32;
    // Mode 2 reads cmvs.cfg from the selected native save root. External
    // Documents/AppData roots remain unbound, just like storage command 22.
    if mode == 2 && matches!(save_root, 1 | 2) {
        return Err(invalid(
            "SHINKI_EMU_CMVS_CONFIG_ROOT_UNBOUND",
            "external native configuration root is not bound",
        ));
    }
    Ok((in_registry, key))
}

fn resolve_windowed(root: &Path, registry: &impl Registry) -> Result<bool, CoreError> {
    let (in_registry, key) = configuration_source(root, registry)?;
    if in_registry {
        let key = format!("{}\\param", key.trim_end_matches('\\'));
        if let Some(mode) = registry.number(&key, "WS_WindowMode")? {
            return Ok(mode != 0);
        }
    }
    Ok(integer(
        &read_ini(root, "cmvs.cfg")?,
        "WINDOW_STATUS",
        "WINDOW_MODE",
        0,
    )? != 0)
}

fn resolve_audio_configuration(
    root: &Path,
    registry: &impl Registry,
) -> Result<([u32; 2], [u32; 2]), CoreError> {
    let (in_registry, key) = configuration_source(root, registry)?;
    let ini = read_ini(root, "cmvs.cfg")?;
    let parameter_key = in_registry.then(|| format!("{}\\param", key.trim_end_matches('\\')));
    let number = |name: &str, registry_name: &str, default| -> Result<u32, CoreError> {
        if let Some(key) = parameter_key.as_deref()
            && let Some(value) = registry.number(key, registry_name)?
        {
            return Ok(value);
        }
        integer(&ini, "SOUND_INFO", name, default).map(|value| value as u32)
    };
    let rate = if number("SOUND_QUALITY", "SO_SoundQuality", 0)? != 0 {
        44100
    } else {
        22050
    };
    let buffer = number("SOUND_BUFFER", "SO_SoundBuffer", 0)?.min(3);
    let bytes = [6144, 8192, 12288, 16384][buffer as usize];
    let se = u32::from(number("SE_USE", "SO_SeUse", 1)? != 0);
    let voice = u32::from(number("VOICE_USE", "SO_VoiceUse", 1)? != 0);
    Ok((
        [rate, if rate == 22050 { bytes / 2 } else { bytes }],
        [voice, se],
    ))
}

fn resolve_view_policy(root: &Path, registry: &impl Registry) -> Result<[u32; 2], CoreError> {
    let (in_registry, key) = configuration_source(root, registry)?;
    let ini = read_ini(root, "cmvs.cfg")?;
    let number = |name: &str, registry_name: &str, default| {
        if in_registry {
            let key = format!("{}\\param", key.trim_end_matches('\\'));
            if let Some(value) = registry.number(&key, registry_name)? {
                return Ok(value);
            }
        }
        integer(&ini, "DISPLAY_CONFIG", name, default).map(|value| value as u32)
    };
    let screen = number("ASPECT_RATIO_SCREEN", "DC_AspectRatioScreen", 1)?;
    let point = number("ASPECT_RATIO_POINT", "DC_AspectRatioPoint", 2)?;
    Ok([
        if screen <= 3 { screen } else { 1 },
        if point <= 4 { point } else { 2 },
    ])
}

/// `429350` is independent of BASE (`429300`) and MODE (`42E870`).
/// `412180` negotiates saved MODE candidates against the selected adapter;
/// that list is not exposed by the family host, so it stays explicitly unbound.
fn resolve_fullscreen_size(
    root: &Path,
    registry: &impl Registry,
) -> Result<Option<[u32; 2]>, CoreError> {
    let (in_registry, key) = configuration_source(root, registry)?;
    let ini = read_ini(root, "cmvs.cfg")?;
    let parameter_key = in_registry.then(|| format!("{}\\param", key.trim_end_matches('\\')));
    let selected = parameter_key
        .as_deref()
        .map(|key| registry.string(key, "CR_Use"))
        .transpose()?
        .flatten()
        .or_else(|| string(&ini, "WINDOW_RESOLVING", "USE"))
        .unwrap_or_else(|| "0,0".into());
    let indices = display_pair(&selected)?;
    if indices[0] != 0 && indices[1] != 0 {
        return Ok(None);
    }
    let number = |section: &str, name: &str, registry_name: &str, default| {
        if let Some(key) = parameter_key.as_deref()
            && let Some(value) = registry.number(key, registry_name)?
        {
            return Ok(value as i32);
        }
        integer(&ini, section, name, default)
    };
    let size = match number("CMVS_SYSTEM_MAIN", "WINDOW_SIZE", "CS_WindowSize", 1)? {
        0 => [640, 480],
        2 => [1024, 768],
        9 => [
            number(
                "RESOLVING_POWER_INFO",
                "FULL_SCREEN_WIDTH",
                "RP_FullScreenWidth",
                800,
            )?,
            number(
                "RESOLVING_POWER_INFO",
                "FULL_SCREEN_HEIGHT",
                "RP_FullScreenHeight",
                600,
            )?,
        ],
        _ => [800, 600],
    };
    if size.iter().any(|value| !(1..=16384).contains(value)) {
        return Err(failure());
    }
    Ok(Some(size.map(|value| value as u32)))
}

fn resolve_display_sizes(
    root: &Path,
    registry: &impl Registry,
) -> Result<([u32; 2], [u32; 2]), CoreError> {
    let (in_registry, key) = configuration_source(root, registry)?;
    let ini = read_ini(root, "cmvs.cfg")?;
    let parameter_key = in_registry.then(|| format!("{}\\param", key.trim_end_matches('\\')));
    let number = |section: &str, name: &str, registry_name: &str, default| {
        if let Some(key) = parameter_key.as_deref()
            && let Some(value) = registry.number(key, registry_name)?
        {
            return Ok(value as i32);
        }
        integer(&ini, section, name, default)
    };
    let selected = parameter_key
        .as_deref()
        .map(|key| registry.string(key, "CR_Use"))
        .transpose()?
        .flatten()
        .or_else(|| string(&ini, "WINDOW_RESOLVING", "USE"))
        .unwrap_or_else(|| "0,0".into());
    let indices = display_pair(&selected)?;
    if indices[0] != 0 && indices[1] != 0 {
        let logical = display_pair(&string(&ini, "WINDOW_RESOLVING", "BASE").ok_or_else(failure)?)?;
        let mode = string(&ini, "WINDOW_RESOLVING", &format!("MODE{:02}", indices[0]))
            .ok_or_else(failure)?;
        let mut dimensions = mode.split(',');
        let width = dimensions.next().ok_or_else(failure)?;
        let height = dimensions.next().ok_or_else(failure)?;
        let render = [
            width.trim().parse::<u32>().map_err(|_| failure())?,
            height.trim().parse::<u32>().map_err(|_| failure())?,
        ];
        if logical.contains(&0) || render.iter().any(|value| !(1..=16384).contains(value)) {
            return Err(failure());
        }
        return Ok((logical, render));
    }
    if number("WINDOW_RESOLVING", "SELECT", "CR_Select", 0)? != 0 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_DISPLAY_MODE_UNBOUND",
            "native display selection has no saved mode",
        ));
    }
    let size = match number("CMVS_SYSTEM_MAIN", "WINDOW_SIZE", "CS_WindowSize", 1)? {
        0 => [640, 480],
        2 => [1024, 768],
        9 => [
            number(
                "RESOLVING_POWER_INFO",
                "WINDOW_WIDTH",
                "RP_WindowWidth",
                800,
            )?,
            number(
                "RESOLVING_POWER_INFO",
                "WINDOW_HEIGHT",
                "RP_WindowHeight",
                600,
            )?,
        ],
        _ => [800, 600],
    };
    if size.iter().any(|value| !(1..=16384).contains(value)) {
        return Err(failure());
    }
    let size = size.map(|value| value as u32);
    Ok((size, size))
}

fn display_pair(value: &str) -> Result<[u32; 2], CoreError> {
    let mut numbers = value.split(',');
    let x = numbers
        .next()
        .ok_or_else(failure)?
        .trim()
        .parse()
        .map_err(|_| failure())?;
    let y = numbers
        .next()
        .ok_or_else(failure)?
        .trim()
        .parse()
        .map_err(|_| failure())?;
    if numbers.next().is_some() || x > 16384 || y > 16384 {
        return Err(failure());
    }
    Ok([x, y])
}

fn resolve_font(root: &Path, registry: &impl Registry) -> Result<String, CoreError> {
    let (in_registry, key) = configuration_source(root, registry)?;
    let configured = if in_registry {
        let key = key.trim_end_matches('\\');
        registry.string(&format!("{key}\\param"), "MI_Font")?
    } else {
        None
    };
    let font = match configured {
        Some(font) => font,
        None => string(&read_ini(root, "cmvs.cfg")?, "MESSAGE_INFO", "FONT").unwrap_or_default(),
    };
    let font = if font.is_empty() {
        DEFAULT_FONT.to_owned()
    } else {
        font
    };
    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&font);
    if invalid || bytes.len() >= 64 || bytes.contains(&0) {
        return Err(failure());
    }
    Ok(font)
}

fn string(ini: &Ini, section: &str, key: &str) -> Option<String> {
    ini.get(section, key).map(|value| {
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value[1..value.len() - 1].to_owned()
        } else {
            value
        }
    })
}

fn integer(ini: &Ini, section: &str, key: &str, default: i32) -> Result<i32, CoreError> {
    string(ini, section, key).map_or(Ok(default), |value| value.parse().map_err(|_| failure()))
}

fn read_ini(root: &Path, filename: &str) -> Result<Ini, CoreError> {
    let mut ini = Ini::new();
    ini.set_comment_symbols(&[';']);
    ini.set_inline_comment_symbols(None);
    ini.set_multiline(false);
    ini.set_cascade_defaults(false);
    let root = root.canonicalize().map_err(|_| failure())?;
    let path = root.join(filename);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ini),
        Err(_) => return Err(failure()),
        Ok(_) => {}
    }
    let path = path.canonicalize().map_err(|_| failure())?;
    if !path.starts_with(&root) {
        return Err(failure());
    }
    let file = File::open(path).map_err(|_| failure())?;
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(failure());
    }
    let text = if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        encoding_rs::UTF_16LE.decode_without_bom_handling_and_without_replacement(bytes)
    } else {
        encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(&bytes)
    }
    .ok_or_else(failure)?;
    if text.contains('\0') {
        return Err(failure());
    }
    ini.read(text.into_owned()).map_err(|_| failure())?;
    Ok(ini)
}

#[cfg(test)]
mod tests;
