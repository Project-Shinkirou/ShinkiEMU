use crate::{CoreError, MusicaMountedVfs, MusicaPazDecryptProvider, PazArchiveConfig};
use shinki_emu_musica_format::into_schemes;
use shinki_emu_musica_keys::{
    KeyFileError, MAX_KEY_BYTES, MAX_TEXTURE_OVERRIDE_BYTES, MusicaKeyFile, key_file_error,
    parse_key_file,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type MusicaTextureOverrides = BTreeMap<String, PathBuf>;

pub(crate) struct TextureVariant {
    pub(crate) source: String,
    pub(crate) path: PathBuf,
}

fn core_error(e: KeyFileError) -> CoreError {
    CoreError::invalid(e.code, e.message)
}

fn configured_key_error(error: CoreError) -> CoreError {
    key_file_error(error.code()).map_or(error, core_error)
}

/// Opens canonical key.toml and archives owned by the Musica core.
pub fn mount_musica(game_root: &Path, profile_path: &Path) -> Result<MusicaMountedVfs, CoreError> {
    mount_musica_with_texture_overrides(game_root, profile_path).map(|(archive, _)| archive)
}

/// Resource paths remain confined to the game even for an external key.toml.
pub(crate) fn mount_musica_with_texture_overrides(
    game_root: &Path,
    profile_path: &Path,
) -> Result<(MusicaMountedVfs, MusicaTextureOverrides), CoreError> {
    mount_musica_with_resources(
        game_root,
        profile_path,
        &crate::MusicaResourcePaths::default(),
    )
}

/// Mount standard PAZ readers with explicit physical script/system aliases.
pub fn mount_musica_with_resources(
    game_root: &Path,
    profile_path: &Path,
    resources: &crate::MusicaResourcePaths,
) -> Result<(MusicaMountedVfs, MusicaTextureOverrides), CoreError> {
    let (bytes, hash, root) =
        shinki_emu_sdk::read_configured_profile_bytes(game_root, profile_path, MAX_KEY_BYTES)
            .map_err(configured_key_error)?;
    let key = parse_key_file(&bytes).map_err(core_error)?;
    let texture_overrides = resolve_texture_overrides(&key, &root)?;
    let configs = key
        .archive_keys
        .iter()
        .filter_map(|(role, archive_key)| {
            let alias = match role.as_str() {
                "scr" => resources.scr_archive.as_str(),
                "sys" => resources.sys_archive.as_str(),
                _ => "",
            };
            let path = if alias.is_empty() {
                root.join(format!("{role}.paz"))
            } else {
                match crate::resources::resource_file(&root, alias) {
                    Ok(path) => path,
                    Err(error) => return Some(Err(error)),
                }
            };
            match path.try_exists() {
                Ok(false) if !archive_key.required => return None,
                Ok(false) => {
                    return Some(Err(CoreError::invalid(
                        "SHINKI_EMU_MUSICA_ARCHIVE_MISSING",
                        format!(
                            "required archive role {role} is missing: {}",
                            path.display()
                        ),
                    )));
                }
                Err(_) => {
                    return Some(Err(CoreError::invalid(
                        "SHINKI_EMU_MUSICA_ARCHIVE_PATH",
                        format!("archive role {role} is inaccessible"),
                    )));
                }
                Ok(true) => {}
            }
            Some(Ok(PazArchiveConfig {
                role: role.clone(),
                path,
                game_root: root.clone(),
                version: key.paz_version,
                index_size_xor: key.index_size_xor,
            }))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    let decrypt = MusicaPazDecryptProvider::new(hash, into_schemes(&key))?;
    let mut archive =
        MusicaMountedVfs::mount("musica.local", "musica:/", configs, Arc::new(decrypt), hash)?;
    archive.set_script_directory(&root, &resources.script_directory)?;
    archive.name_resources = crate::MusicaNameResources::load(&root, &resources.script_directory)?;
    for (uri, variant) in key.texture_variants {
        archive.validate_texture_variant(&uri, &variant.source)?;
        let path = resolve_texture_file(&root, &variant.path)?;
        if texture_overrides.values().any(|other| *other == path)
            || archive
                .texture_variants
                .values()
                .any(|other| other.path == path)
        {
            return Err(invalid(
                "SHINKI_EMU_MUSICA_TEXTURE_VARIANT_COLLISION",
                "texture variant file resolves to another texture file",
            ));
        }
        archive.texture_variants.insert(
            uri,
            TextureVariant {
                source: variant.source,
                path,
            },
        );
    }
    Ok((archive, texture_overrides))
}

pub(crate) fn resolve_texture_overrides(
    key: &MusicaKeyFile,
    root: &Path,
) -> Result<MusicaTextureOverrides, CoreError> {
    key.texture_overrides
        .iter()
        .map(|(source, replacement)| {
            resolve_texture_file(root, replacement).map(|path| (source.clone(), path))
        })
        .collect()
}

fn resolve_texture_file(root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    let path_error = || {
        invalid(
            "SHINKI_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
            "texture replacement path is missing or unsafe",
        )
    };
    let path = shinki_emu_sdk::resolve_game_file(root, relative).map_err(|_| path_error())?;
    let metadata = fs::metadata(&path).map_err(|_| path_error())?;
    if metadata.len() == 0 || metadata.len() > MAX_TEXTURE_OVERRIDE_BYTES {
        return Err(invalid(
            "SHINKI_EMU_MUSICA_TEXTURE_OVERRIDE_BOUND",
            "texture replacement exceeds the supported size",
        ));
    }
    Ok(path)
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests;
