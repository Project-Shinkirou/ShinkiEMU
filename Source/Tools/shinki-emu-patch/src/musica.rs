//! Offline native-reader bridge. All native reads use the transaction's staging tree,
//! never an ambient game path supplied by a recipe. Writes retain capability checks.
use crate::{
    PatchError, PatchErrorKind, PatchProgress, buffer, error::check_cancelled,
    paths::AuthorizedRoot,
};
use shinki_emu_musica::{MusicaResourcePaths, MusicaTranslationTable, ScriptEncoding};
use shinki_emu_musica_keys::{MAX_KEY_BYTES, MusicaTextureVariant, parse_key_file};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
};

pub(crate) fn validate_title_frontend(
    root: &Path,
    archive_root: &Path,
    profile: &str,
    frontend: &str,
    encoding: ScriptEncoding,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> Result<(), PatchError> {
    check_cancelled(cancelled)?;
    crate::paths::relative(profile, false)?;
    let archive = shinki_emu_musica::mount_musica(archive_root, Path::new(profile)).map_err(native_error)?;
    shinki_emu_musica::validate_musica_title_frontend(
        root, &archive, frontend, encoding, width, height,
    )
    .map_err(|error| PatchError::with_source(
        PatchErrorKind::Integrity, "invalid generated Musica title frontend", error,
    ))?;
    check_cancelled(cancelled)
}

pub(crate) struct MusicaEmbedConfig<'a, Progress> {
    pub root: &'a Path,
    pub output: &'a AuthorizedRoot,
    pub profile: &'a str,
    pub translations: &'a [u8],
    pub resources: &'a MusicaResourcePaths,
    pub destination: &'a str,
    pub cancelled: &'a AtomicBool,
    pub progress: &'a Progress,
}

pub(crate) fn embed(
    config: MusicaEmbedConfig<'_, impl Fn(PatchProgress) + Send + Sync>,
) -> Result<usize, PatchError> {
    let MusicaEmbedConfig {
        root,
        output,
        profile,
        translations,
        resources,
        destination,
        cancelled,
        progress,
    } = config;
    crate::paths::relative(profile, false)?;
    crate::paths::relative(destination, false)?;
    let table = MusicaTranslationTable::from_json(translations).map_err(native_error)?;
    let (archive, _) =
        shinki_emu_musica::mount_musica_with_resources(root, Path::new(profile), resources)
            .map_err(native_error)?;
    let scripts: Vec<_> = archive
        .manifest()
        .entries
        .iter()
        .filter(|entry| {
            entry.uri.starts_with("musica:/scr/") && entry.uri.to_ascii_lowercase().ends_with(".sc")
        })
        .collect();
    if scripts.is_empty() {
        return Err(PatchError::new(
            PatchErrorKind::UnsupportedInput,
            "native script archive is empty",
        ));
    }
    output.mkdir(destination)?;
    let mut replaced = 0;
    for (index, entry) in scripts.iter().enumerate() {
        check_cancelled(cancelled)?;
        let name = entry
            .uri
            .strip_prefix("musica:/scr/")
            .expect("filtered native script URI");
        crate::paths::relative(name, false)?;
        // Musica's native loose-SC consumer is flat; reject rather than silently
        // generating an unconsumed nested script.
        if name.contains('/') {
            return Err(PatchError::new(
                PatchErrorKind::UnsupportedInput,
                "native loose scripts require flat names",
            ));
        }
        let source =
            shinki_emu_musica::read_expanded_script(&archive, &entry.uri).map_err(|_| {
                PatchError::new(PatchErrorKind::Codec, "native script cannot be decoded")
            })?;
        buffer::bounded(source.len())?;
        let (bytes, hits) =
            shinki_emu_musica::embed_sc_localization(&source, &table, ScriptEncoding::Gbk)
                .map_err(native_error)?;
        buffer::bounded(bytes.len())?;
        let path = format!("{destination}/{name}");
        let mut file = output.create(&path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        replaced += hits;
        progress(PatchProgress {
            phase: "Embedding native Musica scripts".into(),
            completed: index as u64 + 1,
            total: Some(scripts.len() as u64),
            ..Default::default()
        });
    }
    if replaced == 0 {
        return Err(PatchError::new(
            PatchErrorKind::UnsupportedInput,
            "translation has no native script display endpoints",
        ));
    }
    Ok(replaced)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextureVariantSpec {
    source: String,
    path: String,
    conversion: MonoConversion,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum MonoConversion {
    CnbgMono,
    NativeMono,
}

pub(crate) fn generate_texture_variants(
    root: &Path,
    output: &AuthorizedRoot,
    profile_path: &str,
    resources: &MusicaResourcePaths,
    specs: BTreeMap<String, TextureVariantSpec>,
    cancelled: &AtomicBool,
) -> Result<(), PatchError> {
    check_cancelled(cancelled)?;
    let mut profile_bytes = Vec::new();
    output
        .open_read(profile_path)?
        .take(MAX_KEY_BYTES + 1)
        .read_to_end(&mut profile_bytes)?;
    let mut profile = parse_key_file(&profile_bytes).map_err(|error| {
        PatchError::with_source(PatchErrorKind::Integrity, "invalid staged Musica profile", error)
    })?;
    if specs.is_empty() || !profile.texture_variants.is_empty() {
        return Err(PatchError::new(
            PatchErrorKind::Integrity,
            "texture variant declarations must be supplied once",
        ));
    }
    let (archive, _) =
        shinki_emu_musica::mount_musica_with_resources(root, Path::new(profile_path), resources)
            .map_err(native_error)?;
    for (uri, spec) in &specs {
        check_cancelled(cancelled)?;
        crate::paths::relative(&spec.path, false)?;
        profile.texture_variants.insert(
            uri.clone(),
            MusicaTextureVariant {
                source: spec.source.clone(),
                path: spec.path.clone(),
            },
        );
    }
    profile.validate().map_err(|error| {
        PatchError::with_source(PatchErrorKind::Integrity, "invalid texture variant declarations", error)
    })?;
    for (uri, spec) in &specs {
        check_cancelled(cancelled)?;
        archive
            .validate_texture_variant(uri, &spec.source)
            .map_err(native_error)?;
    }
    let encoded = toml::to_string(&profile)
        .map_err(|_| PatchError::new(PatchErrorKind::Codec, "Musica profile encoding failed"))?;
    if encoded.len() as u64 > MAX_KEY_BYTES {
        return Err(PatchError::new(
            PatchErrorKind::Bounds,
            "Musica profile exceeds its size limit",
        ));
    }
    for spec in specs.into_values() {
        check_cancelled(cancelled)?;
        let (width, height, mut rgba) = match spec.conversion {
            MonoConversion::CnbgMono => {
                let path = profile.texture_overrides.get(&spec.source).ok_or_else(|| {
                    PatchError::new(
                        PatchErrorKind::Integrity,
                        "cnbg variant requires its converted bitmap",
                    )
                })?;
                let mut png = Vec::new();
                output
                    .open_read(path)?
                    .take(buffer::MAX_NATIVE_BYTES as u64 + 1)
                    .read_to_end(&mut png)?;
                crate::data::decode_png(&png, cancelled)?
            }
            MonoConversion::NativeMono => {
                // This mode always reads PAZ, even if the base URI has an override.
                let source = archive.entry_descriptor(&spec.source).map_err(native_error)?;
                let size = usize::try_from(source.unpacked_size).unwrap_or(usize::MAX);
                buffer::bounded(size)?;
                let png = archive
                    .read_range(&spec.source, 0, source.unpacked_size)
                    .map_err(native_error)?;
                crate::data::decode_png(png.bytes.as_ref(), cancelled)?
            }
        };
        mono_pixels(&mut rgba, spec.conversion, cancelled)?;
        let png = crate::data::encode_png(width, height, &rgba, cancelled)?;
        if let Some((parent, _)) = spec.path.rsplit_once('/') {
            output.mkdir(parent)?;
        }
        let mut file = output.create(&spec.path)?;
        file.write_all(&png)?;
        file.sync_all()?;
    }
    check_cancelled(cancelled)?;
    let mut file = output.edit(profile_path)?;
    file.write_all(encoded.as_bytes())?;
    file.set_len(encoded.len() as u64)?;
    file.sync_all()?;
    Ok(())
}

fn mono_pixels(
    rgba: &mut [u8],
    conversion: MonoConversion,
    cancelled: &AtomicBool,
) -> Result<(), PatchError> {
    for chunk in rgba.chunks_mut(64 * 1024) {
        check_cancelled(cancelled)?;
        for pixel in chunk.chunks_exact_mut(4) {
            let (r, g, b) = (u16::from(pixel[0]), u16::from(pixel[1]), u16::from(pixel[2]));
            let gray = match conversion {
                MonoConversion::CnbgMono => (30 * r + 59 * g + 11 * b) / 100,
                MonoConversion::NativeMono if pixel[3] != 0 => (5 * r + 2 * g + b) >> 3,
                MonoConversion::NativeMono => continue,
            } as u8;
            pixel[..3].fill(gray);
        }
    }
    Ok(())
}

fn native_error(error: shinki_emu_musica::CoreError) -> PatchError {
    PatchError::with_source(
        PatchErrorKind::Codec,
        "native Musica conversion failed",
        error,
    )
}
