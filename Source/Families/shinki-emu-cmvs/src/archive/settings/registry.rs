use super::*;

/// Portable store for the values the original engine keeps under
/// `HKEY_CURRENT_USER\Software\<REG>` (32-bit view). It lives in the game
/// directory as `.shinki/cmvs/registry.json`:
///
/// ```json
/// { "Vendor\\title": { "ConfigPut": 1 },
///   "Vendor\\title\\param": { "MI_Font": "ＭＳ ゴシック", "MI_TextBold": 0 } }
/// ```
///
/// Numbers are `REG_DWORD` values and strings are `REG_SZ` values. Key and
/// value names compare case-insensitively, as registry lookups do. A missing
/// file, key or value reads as absent; a value of the other type fails like
/// `RegGetValueW` with a mismatched `RRF_RT_*` filter.
pub(super) struct PortableRegistry<'a> {
    root: &'a Path,
}

pub(super) const REGISTRY_FILE: &str = ".shinki/cmvs/registry.json";
const MAX_REGISTRY_BYTES: u64 = 1024 * 1024;

impl<'a> PortableRegistry<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self { root }
    }

    fn read(&self, key: &str, value: &str) -> Result<Option<serde_json::Value>, CoreError> {
        let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(key);
        if key.is_empty()
            || encoded.len() >= 64 - "Software\\".len()
            || invalid
            || key.contains('\0')
        {
            return Err(failure());
        }
        let Some(store) = load(self.root)? else {
            return Ok(None);
        };
        let key = key.trim_end_matches('\\');
        let entry = store
            .iter()
            .find(|(name, _)| name.trim_end_matches('\\').eq_ignore_ascii_case(key))
            .map(|(_, values)| values);
        let Some(values) = entry else {
            return Ok(None);
        };
        let values = values.as_object().ok_or_else(failure)?;
        Ok(values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(value))
            .map(|(_, value)| value.clone()))
    }
}

fn load(root: &Path) -> Result<Option<serde_json::Map<String, serde_json::Value>>, CoreError> {
    let root = root.canonicalize().map_err(|_| failure())?;
    let path = root.join(REGISTRY_FILE);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure()),
        Ok(_) => {}
    }
    let path = path.canonicalize().map_err(|_| failure())?;
    if !path.starts_with(&root) {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| failure())?
        .take(MAX_REGISTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 > MAX_REGISTRY_BYTES {
        return Err(failure());
    }
    match serde_json::from_slice(&bytes).map_err(|_| failure())? {
        serde_json::Value::Object(store) => Ok(Some(store)),
        _ => Err(failure()),
    }
}

impl Registry for PortableRegistry<'_> {
    fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError> {
        self.read(key, value)?
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(failure)
            })
            .transpose()
    }
    fn string(&self, key: &str, value: &str) -> Result<Option<String>, CoreError> {
        self.read(key, value)?
            .map(|value| {
                let text = value.as_str().ok_or_else(failure)?;
                // RegGetValueW reads at most 512 bytes of UTF-16 with its NUL.
                if text.contains('\0') || text.encode_utf16().count() >= 256 {
                    return Err(failure());
                }
                Ok(text.to_owned())
            })
            .transpose()
    }
}
