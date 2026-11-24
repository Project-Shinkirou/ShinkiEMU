use crate::{
    Error, Result,
    binary::{c_string, decode_japanese, dword, range},
    mes::{MAX_SCRIPT_BYTES, Script},
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct Game {
    pub(crate) id: String,
    pub(crate) data: PathBuf,
    pub(crate) startup: Arc<Script>,
    assets: BTreeMap<String, PathBuf>,
}

#[derive(Default)]
pub(crate) struct SoundNames {
    system: Vec<String>,
    music: Vec<String>,
    effects: Vec<String>,
    pictures: Vec<String>,
    scenarios: Vec<String>,
}

impl SoundNames {
    pub(crate) fn system(&self, index: usize) -> Option<&str> {
        self.system
            .get(index)
            .filter(|name| !name.is_empty())
            .map(String::as_str)
    }

    pub(crate) fn music(&self, index: u16) -> Result<&str> {
        resource_name(&self.music, index)
    }

    pub(crate) fn effect(&self, index: u16) -> Result<&str> {
        resource_name(&self.effects, index)
    }

    pub(crate) fn picture(&self, index: u16) -> Result<&str> {
        resource_name(&self.pictures, index)
    }

    pub(crate) fn picture_index(&self, name: &str) -> Option<usize> {
        let name = name.split('.').next().unwrap_or(name).replace('/', "\\");
        self.pictures
            .iter()
            .position(|entry| entry.eq_ignore_ascii_case(&name))
    }

    pub(crate) fn scenario(&self, name: &str) -> Option<usize> {
        let name = name.split('.').next().unwrap_or(name);
        self.scenarios
            .iter()
            .position(|entry| entry.eq_ignore_ascii_case(name))
    }
}

fn resource_name(values: &[String], index: u16) -> Result<&str> {
    values
        .get(usize::from(index))
        .filter(|name| !name.is_empty())
        .map(String::as_str)
        .ok_or(Error::Invalid("ADV32 sound number"))
}

impl Game {
    pub(crate) fn text_encoding(&self) -> &'static encoding_rs::Encoding {
        let has_gbk = self
            .assets
            .range("mes/".to_owned()..)
            .take_while(|(name, _)| name.starts_with("mes/"))
            .filter(|(name, _)| name.ends_with(".mes"))
            .take(48)
            .filter_map(|(_, path)| {
                read_bounded(path, MAX_SCRIPT_BYTES)
                    .and_then(Script::parse)
                    .ok()
            })
            .filter(|script| script.dialect == self.startup.dialect)
            .any(|script| script.has_gbk_dialogue());
        if has_gbk {
            encoding_rs::GBK
        } else {
            encoding_rs::SHIFT_JIS
        }
    }

    pub(crate) fn find(root: &Path) -> Result<Option<Self>> {
        let Some(data) = find_directory(root, "AdvData")? else {
            return Ok(None);
        };
        let Some(mes) = find_directory(&data, "MES")? else {
            return Ok(None);
        };
        // "start" in AdvData/MES is the executable's bootstrap convention.
        let Some(startup) = find_file(&mes, "start.mes")? else {
            return Ok(None);
        };
        let startup = Arc::new(Script::parse(read_bounded(&startup, MAX_SCRIPT_BYTES)?)?);
        let id = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("ADV32")
            .to_owned();
        let mut assets = BTreeMap::new();
        for kind in ["MES", "GRP", "MUSIC", "SE", "MOVIE", "VOICE"] {
            if let Some(directory) = find_directory(&data, kind)? {
                index_directory(&directory, &kind.to_ascii_lowercase(), &mut assets)?;
            }
        }
        if let Some(voice) = find_file(&data, "VOICE.DAT")? {
            assets.insert("voice.dat".into(), voice);
        }
        Ok(Some(Self {
            id,
            data,
            startup,
            assets,
        }))
    }

    pub(crate) fn asset(&self, name: &str) -> Result<&Path> {
        let key = resource_key(name)?;
        self.assets
            .get(&key)
            .map(PathBuf::as_path)
            .ok_or(Error::MissingAsset(key))
    }

    pub(crate) fn script(&self, name: &str) -> Result<Script> {
        let path = self.resource("mes", name, "mes")?;
        let script = Script::parse(read_bounded(path, MAX_SCRIPT_BYTES)?)?;
        if script.dialect != self.startup.dialect {
            return Err(Error::UnsupportedMesVersion(script.dialect.version));
        }
        Ok(script)
    }

    pub(crate) fn resource(&self, kind: &str, name: &str, suffix: &str) -> Result<&Path> {
        // A single separator anchors a resource at its virtual group root.
        let name = name.strip_prefix(['/', '\\']).unwrap_or(name);
        let name = resource_key(name)?;
        let name = if name
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|leaf| leaf.contains('.'))
        {
            format!("{kind}/{name}")
        } else {
            format!("{kind}/{name}.{suffix}")
        };
        self.asset(&name)
    }

    pub(crate) fn sound_names(&self) -> Result<SoundNames> {
        let Ok(path) = self.asset("grp/names.dat") else {
            return Ok(SoundNames::default());
        };
        let bytes = read_bounded(path, 2 * 1024 * 1024)?;
        let mut at = 0;
        let system = sound_table(&bytes, &mut at, 32, 32)?;
        let music = sound_table(&bytes, &mut at, 48, 32)?;
        let effects = sound_table(&bytes, &mut at, 36, 36)?;
        let pictures = sound_table(&bytes, &mut at, 32, 32)?;
        let scenarios = sound_table(&bytes, &mut at, 32, 32)?;
        Ok(SoundNames {
            system,
            music,
            effects,
            pictures,
            scenarios,
        })
    }
}

fn skip_table<'a>(data: &'a [u8], at: &mut usize, stride: usize) -> Result<&'a [u8]> {
    let count = dword(data, *at)? as usize;
    let size = count
        .checked_mul(stride)
        .ok_or(Error::Invalid("ADV32 names directory"))?;
    let table = range(data, *at + 4, size)?;
    *at += 4 + size;
    Ok(table)
}

fn sound_table(
    data: &[u8],
    at: &mut usize,
    stride: usize,
    name_bytes: usize,
) -> Result<Vec<String>> {
    skip_table(data, at, stride)?
        .chunks_exact(stride)
        .map(|record| {
            let name = &record[..name_bytes];
            decode_japanese(c_string(name).unwrap_or(name))
        })
        .collect()
}

fn resource_key(name: &str) -> Result<String> {
    let separators = ['/', '\\'];
    if name.is_empty()
        || name.starts_with(separators)
        || name.ends_with(separators)
        || name
            .split(separators)
            .any(|part| matches!(part, "." | "..") || part.contains(':'))
    {
        return Err(Error::Invalid("ADV32 asset name"));
    }
    Ok(name
        .split(separators)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
        .to_ascii_lowercase())
}

pub(crate) fn find_directory(parent: &Path, name: &str) -> Result<Option<PathBuf>> {
    find_child(parent, name, true)
}

pub(crate) fn find_file(parent: &Path, name: &str) -> Result<Option<PathBuf>> {
    find_child(parent, name, false)
}

fn find_child(parent: &Path, name: &str, directory: bool) -> Result<Option<PathBuf>> {
    let mut found = None;
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if !entry.file_name().eq_ignore_ascii_case(name) {
            continue;
        }
        let kind = entry.file_type()?;
        if (directory && kind.is_dir()) || (!directory && kind.is_file()) {
            if found.replace(entry.path()).is_some() {
                return Err(Error::Invalid("ambiguous ADV32 path"));
            }
        }
    }
    Ok(found)
}

fn index_directory(
    root: &Path,
    prefix: &str,
    assets: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| !name.starts_with('.')) else {
            continue;
        };
        let kind = entry.file_type()?;
        let key = resource_key(&format!("{prefix}/{name}"))?;
        if kind.is_dir() {
            index_directory(&entry.path(), &key, assets)?;
        } else if kind.is_file() && assets.insert(key, entry.path()).is_some() {
            return Err(Error::Invalid("ambiguous ADV32 asset"));
        }
    }
    Ok(())
}

pub(crate) fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    if length > max as u64 {
        return Err(Error::Invalid("ADV32 asset length"));
    }
    let mut data = Vec::with_capacity(length as usize);
    file.take(max as u64 + 1).read_to_end(&mut data)?;
    if data.len() > max {
        return Err(Error::Invalid("ADV32 asset length"));
    }
    Ok(data)
}
