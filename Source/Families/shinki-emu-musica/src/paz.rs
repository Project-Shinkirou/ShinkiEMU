use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::archive::{
    ARCHIVE_MANIFEST_SCHEMA, ARCHIVE_MAX_READ_BYTES, ArchiveEntry, ArchiveManifest, ArchiveNode,
    ArchiveNodeKind, ArchiveReadResult, ArchiveSource, ArchiveStat, ArchiveStream,
    PAZ_DECRYPT_CHUNK_BYTES, PAZ_DECRYPT_MAX_BATCH_BYTES, PazDecryptDescriptor, PazDecryptPhase,
    PazDecryptRequest, PazDecryptTransport, validate_archive_directory_uri, validate_archive_uri,
    validate_decrypt_output, validate_decrypt_request,
};
use roukaku_byte_source::OwnedByteBuffer;
use roukaku_core::Hash256;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::archive::{CacheIdentity, PlaintextCache, PlaintextCacheError};

use crate::{ArchiveEntryDescriptor, PazRoleScheme};
use crate::{MUSICA_DECRYPT_DESCRIPTOR_SCHEMA, MUSICA_DECRYPT_PROVIDER_ID, MUSICA_READER_ID};
#[cfg(test)]
use blowfish::Blowfish;
#[cfg(any(test, feature = "ffmpeg-vcpkg"))]
use encoding_rs::SHIFT_JIS;
#[cfg(any(test, feature = "ffmpeg-vcpkg"))]
use flate2::read::ZlibDecoder;
#[cfg(any(test, feature = "ffmpeg-vcpkg"))]
use rc4::{KeyInit, Rc4, StreamCipher};
use shinki_emu_musica_format::paz::{self as format, password_for_entry, xor_byte};
#[cfg(test)]
const FIXTURE_ARCHIVE_ROLES: [&str; 8] = ["bg", "bgm", "scr", "st", "sys", "se", "voice", "mov"];

type PazError = crate::CoreError;

#[cfg(any(feature = "ffmpeg-vcpkg", test))]
mod movie_stream;

#[derive(Debug, Clone)]
pub struct PazArchiveConfig {
    pub role: String,
    pub path: PathBuf,
    pub game_root: PathBuf,
    pub version: u8,
    pub index_size_xor: u32,
}

/// The only production decrypt provider for Musica PAZ archives.
/// Key material remains process-local and is never serializable.
pub struct MusicaPazDecryptProvider {
    private_profile_hash: Hash256,
    roles: BTreeMap<String, PazRoleScheme>,
}

impl MusicaPazDecryptProvider {
    pub fn new(
        private_profile_hash: Hash256,
        roles: BTreeMap<String, PazRoleScheme>,
    ) -> Result<Self, PazError> {
        if roles.is_empty()
            || roles
                .keys()
                .any(|role| !shinki_emu_musica_keys::valid_archive_role(role))
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_DECODER_CONFIG",
                "decoder must declare safe archive roles",
            ));
        }
        for (role, scheme) in &roles {
            format::validate_blowfish_key(&scheme.index_key).map_err(format_error)?;
            if role != "mov" {
                format::validate_blowfish_key(&scheme.data_key).map_err(format_error)?;
            } else if scheme.data_key.len() > 56 {
                return Err(error(
                    "SHINKI_EMU_MUSICA_BLOWFISH_KEY",
                    "movie data key exceeds the supported bound",
                ));
            }
        }
        Ok(Self {
            private_profile_hash,
            roles,
        })
    }

    fn scheme(&self, role: &str) -> Result<&PazRoleScheme, PazError> {
        self.roles.get(role).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_DECODER_ROLE",
                "decoder has no scheme for the archive role",
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum MusicaDecryptDescriptor {
    Index {
        role: String,
        version: u8,
        stream_offset: u64,
    },
    Entry {
        version: u8,
        entry: ArchiveEntryDescriptor,
        stream_offset: u64,
    },
}

impl MusicaPazDecryptProvider {
    pub fn provider_id(&self) -> &str {
        MUSICA_DECRYPT_PROVIDER_ID
    }
    pub fn private_profile_hash(&self) -> Hash256 {
        self.private_profile_hash
    }
    pub fn descriptor_schema_id(&self) -> &str {
        MUSICA_DECRYPT_DESCRIPTOR_SCHEMA
    }
    pub fn descriptor_schema_hash(&self) -> Hash256 {
        Hash256::from_sha256(MUSICA_DECRYPT_DESCRIPTOR_SCHEMA.as_bytes())
    }

    pub fn decrypt(&self, request: PazDecryptRequest<'_>) -> Result<Vec<u8>, PazError> {
        validate_decrypt_request(self, &request)?;
        if request.descriptors.len() != 1 {
            return Err(error(
                "SHINKI_EMU_MUSICA_DESCRIPTOR_BATCH",
                "Musica decrypt batches require exactly one descriptor",
            ));
        }
        let descriptor: MusicaDecryptDescriptor =
            serde_json::from_slice(&request.descriptors[0].payload).map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_DESCRIPTOR",
                    "Musica decrypt descriptor is invalid",
                )
            })?;
        if !matches!(
            (&request.phase, &descriptor),
            (
                PazDecryptPhase::Index,
                MusicaDecryptDescriptor::Index { .. }
            ) | (
                PazDecryptPhase::Entry,
                MusicaDecryptDescriptor::Entry { .. }
            )
        ) {
            return Err(error(
                "SHINKI_EMU_MUSICA_DESCRIPTOR_PHASE",
                "Musica decrypt descriptor phase does not match the request",
            ));
        }
        let absolute_offset = match &descriptor {
            MusicaDecryptDescriptor::Index { stream_offset, .. }
            | MusicaDecryptDescriptor::Entry { stream_offset, .. } => stream_offset
                .checked_add(request.transport.chunk_offset)
                .ok_or_else(|| {
                    error(
                        "SHINKI_EMU_MUSICA_DECRYPT_OFFSET",
                        "decrypt stream offset overflowed",
                    )
                })?,
        };
        let output = match descriptor {
            MusicaDecryptDescriptor::Index { role, .. } => {
                format::blowfish_decrypt(self.scheme(&role)?.index_key.as_slice(), request.bytes)
                    .map_err(format_error)?
            }
            MusicaDecryptDescriptor::Entry { version, entry, .. } => {
                self.decrypt_entry_chunk(version, &entry, absolute_offset, request.bytes)?
            }
        };
        validate_decrypt_output(&request, &output)?;
        if output.len() != request.bytes.len() {
            return Err(error(
                "SHINKI_EMU_MUSICA_DECRYPT_SIZE",
                "Musica transform changed the chunk size",
            ));
        }
        Ok(output)
    }
}

impl MusicaPazDecryptProvider {
    fn decrypt_entry_chunk(
        &self,
        version: u8,
        entry: &ArchiveEntryDescriptor,
        absolute_offset: u64,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, PazError> {
        format::decrypt_entry_chunk(
            self.scheme(&entry.archive_role)?,
            version,
            entry,
            absolute_offset,
            encrypted,
        )
        .map_err(format_error)
    }
}

#[derive(Clone)]
struct ArchiveFile {
    role: String,
    parts: Vec<ArchivePart>,
    version: u8,
    length: u64,
    revision: Hash256,
    xor_key: u8,
}

#[derive(Clone)]
struct ArchivePart {
    path: PathBuf,
    length: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone)]
struct MountedEntry {
    descriptor: ArchiveEntryDescriptor,
    uri: String,
    archive: usize,
}

pub struct MusicaMountedVfs {
    mount_id: String,
    prefix: String,
    manifest: ArchiveManifest,
    archives: Vec<ArchiveFile>,
    entries: BTreeMap<String, MountedEntry>,
    decrypt_provider: Arc<MusicaPazDecryptProvider>,
    cache: Option<PlaintextCache>,
    pub(crate) loose_scripts: BTreeMap<String, (PathBuf, String)>,
    pub(crate) name_resources: Option<crate::MusicaNameResources>,
    pub(crate) texture_variants: BTreeMap<String, crate::profile::TextureVariant>,
}

impl MusicaMountedVfs {
    pub fn script_uris(&self) -> Vec<String> {
        let mut names: BTreeMap<String, String> = self
            .manifest
            .entries
            .iter()
            .filter(|entry| {
                entry.uri.starts_with("musica:/scr/")
                    && entry.uri.to_ascii_lowercase().ends_with(".sc")
            })
            .map(|entry| (entry.uri.to_ascii_lowercase(), entry.uri.clone()))
            .collect();
        for uri in self.loose_scripts.keys() {
            names.insert(uri.to_ascii_lowercase(), uri.clone());
        }
        names.into_values().collect()
    }

    pub(crate) fn set_script_directory(
        &mut self,
        root: &Path,
        directory: &str,
    ) -> Result<(), PazError> {
        if directory.is_empty() {
            return Ok(());
        }
        crate::resources::relative_resource(directory)?;
        let root = root.canonicalize().map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_RESOURCE_PATH",
                "game directory cannot be resolved",
            )
        })?;
        let path = root.join(directory).canonicalize().map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_RESOURCE_PATH",
                "script directory cannot be resolved",
            )
        })?;
        if !path.starts_with(&root) || !path.is_dir() {
            return Err(error(
                "SHINKI_EMU_MUSICA_RESOURCE_PATH",
                "script directory must remain inside the game directory",
            ));
        }
        for entry in std::fs::read_dir(path).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_RESOURCE_READ",
                "script directory cannot be read",
            )
        })? {
            let entry = entry.map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_RESOURCE_READ",
                    "script entry cannot be read",
                )
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_RESOURCE_PATH",
                    "script filename is not Unicode",
                )
            })?;
            if !name.to_ascii_lowercase().ends_with(".sc") {
                continue;
            }
            let relative = format!("{directory}/{name}");
            crate::resources::resource_file(&root, &relative)?;
            let uri = format!("musica:/scr/{name}");
            if self
                .loose_scripts
                .keys()
                .any(|key| key.eq_ignore_ascii_case(&uri))
            {
                return Err(error(
                    "SHINKI_EMU_MUSICA_SCRIPT_AMBIGUOUS",
                    "loose script names collide under native case rules",
                ));
            }
            self.loose_scripts.insert(uri, (root.clone(), relative));
        }
        Ok(())
    }

    /// Stable save identity includes actual loose source bytes, not just PAZ metadata.
    pub fn script_source_identity(&self) -> Result<Hash256, PazError> {
        let mut hash = Sha256::new();
        for (uri, (root, relative)) in &self.loose_scripts {
            hash.update((uri.len() as u64).to_le_bytes());
            hash.update(uri.as_bytes());
            hash.update(
                Hash256::from_sha256(&crate::resources::read_resource(
                    root,
                    relative,
                    16 * 1024 * 1024,
                )?)
                .as_bytes(),
            );
        }
        Ok(Hash256::from_bytes(hash.finalize().into()))
    }

    pub fn mount(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decrypt_provider: Arc<MusicaPazDecryptProvider>,
        mount_profile_hash: Hash256,
    ) -> Result<Self, PazError> {
        Self::mount_with_cache(
            mount_id,
            prefix,
            configs,
            decrypt_provider,
            mount_profile_hash,
            None,
        )
    }

    pub fn mount_with_cache(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decrypt_provider: Arc<MusicaPazDecryptProvider>,
        mount_profile_hash: Hash256,
        cache: Option<PlaintextCache>,
    ) -> Result<Self, PazError> {
        let mount_id = mount_id.into();
        let prefix = prefix.into();
        if prefix != "musica:/" {
            return Err(error(
                "SHINKI_EMU_MUSICA_PREFIX",
                "Musica mounts require the stable musica:/ prefix",
            ));
        }
        validate_role_set(&configs)?;
        let mut archives = Vec::with_capacity(configs.len());
        let mut entries = BTreeMap::new();
        let mut entry_ids = BTreeSet::new();
        for config in configs {
            tracing::info!(
                event = "shinki_emu_musica_archive_mount_started",
                archive_role = %config.role,
                version = config.version
            );
            let parts = discover_parts(&config.path, &config.game_root)?;
            let total_length = parts
                .iter()
                .try_fold(0u64, |total, part| total.checked_add(part.length))
                .ok_or_else(|| {
                    error(
                        "SHINKI_EMU_MUSICA_ARCHIVE_SIZE",
                        "multipart PAZ size overflowed",
                    )
                })?;
            if parts[0].length == 0 {
                return Err(error(
                    "SHINKI_EMU_MUSICA_ARCHIVE_EMPTY",
                    format!("required archive role {} is empty", config.role),
                ));
            }
            if config.version > 2 {
                return Err(error(
                    "SHINKI_EMU_MUSICA_VERSION",
                    "only PAZ versions 0 through 2 are supported",
                ));
            }
            let mut source = ArchiveFile {
                role: config.role.clone(),
                parts,
                version: config.version,
                length: total_length,
                revision: Hash256::from_sha256(&[]),
                xor_key: 0,
            };
            let parsed = parse_archive_index(
                &mut source,
                config.index_size_xor,
                decrypt_provider.as_ref(),
            )?;
            tracing::info!(
                event = "shinki_emu_musica_archive_index_decoded",
                archive_role = %config.role,
                entry_count = parsed.len()
            );
            validate_entry_ranges(source.length, &parsed)?;
            let archive_index = archives.len();
            for entry in parsed {
                let uri = format!(
                    "{}{}/{}",
                    prefix,
                    config.role,
                    normalize_entry_name(&entry.name)?
                );
                validate_archive_uri(&prefix, &uri)?;
                if !entry_ids.insert(entry.entry_id.clone()) || entries.contains_key(&uri) {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_ENTRY_DUPLICATE",
                        "PAZ set contains a duplicate URI or entry id",
                    ));
                }
                entries.insert(
                    uri.clone(),
                    MountedEntry {
                        descriptor: entry,
                        uri,
                        archive: archive_index,
                    },
                );
            }
            archives.push(source);
            tracing::info!(
                event = "shinki_emu_musica_archive_mount_completed",
                archive_role = %config.role
            );
        }
        let reader_material = archives
            .iter()
            .flat_map(|archive| archive.revision.as_bytes().iter().copied())
            .collect::<Vec<_>>();
        let reader_hash = Hash256::from_sha256(&reader_material);
        let manifest_entries = entries
            .values()
            .map(|entry| ArchiveEntry {
                uri: entry.uri.clone(),
                entry_id: entry.descriptor.entry_id.clone(),
                source_id: entry.descriptor.archive_role.clone(),
                source_offset: entry.descriptor.offset,
                stored_size: entry.descriptor.aligned_size,
                decoded_size: entry.descriptor.unpacked_size,
                source_revision: archives[entry.archive].revision,
                content_hash: None,
                method: entry_method(&entry.descriptor).into(),
                media_kind: media_kind(&entry.descriptor.name).into(),
            })
            .collect();
        let manifest = ArchiveManifest {
            schema: ARCHIVE_MANIFEST_SCHEMA.into(),
            family_id: "musica".into(),
            mount_id: mount_id.clone(),
            prefix: prefix.clone(),
            reader_id: MUSICA_READER_ID.into(),
            reader_hash,
            decrypt_provider_id: decrypt_provider.provider_id().into(),
            private_profile_hash: decrypt_provider.private_profile_hash(),
            mount_profile_hash,
            sources: archives
                .iter()
                .map(|archive| ArchiveSource {
                    source_id: archive.role.clone(),
                    archive_role: Some(archive.role.clone()),
                    byte_size: archive.length,
                    part_count: archive.parts.len() as u32,
                    source_revision: archive.revision,
                })
                .collect(),
            entries: manifest_entries,
        };
        manifest.validate(10_000_000)?;
        Ok(Self {
            mount_id,
            prefix,
            manifest,
            archives,
            entries,
            decrypt_provider,
            cache,
            loose_scripts: BTreeMap::new(),
            name_resources: None,
            texture_variants: BTreeMap::new(),
        })
    }

    fn decoded_entry(&self, entry: &MountedEntry) -> Result<(Vec<u8>, bool), PazError> {
        let archive = &self.archives[entry.archive];
        verify_source_unchanged(archive)?;
        let identity = CacheIdentity {
            family_id: "musica".into(),
            source_revision: archive.revision,
            entry_id: entry.descriptor.entry_id.clone(),
            private_profile_hash: self.decrypt_provider.private_profile_hash(),
            decrypt_provider_id: self.decrypt_provider.provider_id().into(),
            descriptor_schema_hash: self.decrypt_provider.descriptor_schema_hash(),
            codec_identity: if entry.descriptor.packed {
                "paz-zlib-v1"
            } else {
                "paz-raw-v1"
            }
            .into(),
        };
        if let Some(bytes) = self
            .cache
            .as_ref()
            .map(|cache| cache.get(&identity))
            .transpose()
            .map_err(cache_error)?
            .flatten()
        {
            if bytes.len() as u64 != entry.descriptor.unpacked_size {
                return Err(error(
                    "SHINKI_EMU_MUSICA_CACHE_SIZE",
                    "cached plaintext size does not match the entry descriptor",
                ));
            }
            return Ok((bytes, true));
        }
        let mut encrypted = read_source_range(
            archive,
            entry.descriptor.offset,
            entry.descriptor.aligned_size,
        )?;
        xor_byte(&mut encrypted, archive.xor_key);
        let mut decoded = decrypt_bytes(
            self.decrypt_provider.as_ref(),
            MusicaDecryptDescriptor::Entry {
                version: archive.version,
                entry: entry.descriptor.clone(),
                stream_offset: 0,
            },
            &encrypted,
        )?;
        decoded = format::finish_entry(&entry.descriptor, decoded).map_err(format_error)?;
        if let Some(cache) = &self.cache {
            cache.put(&identity, &decoded).map_err(cache_error)?;
        }
        Ok((decoded, false))
    }

    fn entry(&self, uri: &str) -> Result<&MountedEntry, PazError> {
        validate_archive_uri(&self.prefix, uri)?;
        self.entries.get(uri).ok_or_else(|| {
            error(
                "SHINKI_EMU_VFS_NOT_FOUND",
                format!("VFS entry was not found: {uri}"),
            )
        })
    }
}

impl MusicaMountedVfs {
    pub fn mount_id(&self) -> &str {
        &self.mount_id
    }
    pub fn manifest(&self) -> &ArchiveManifest {
        &self.manifest
    }

    /// Physical reader metadata for explicit offline inspection; no payload read.
    pub fn entry_descriptor(&self, uri: &str) -> Result<&ArchiveEntryDescriptor, PazError> {
        Ok(&self.entry(uri)?.descriptor)
    }

    /// Validate a supplemental PNG URI against the native PAZ index.
    pub fn validate_texture_variant(&self, uri: &str, source: &str) -> Result<(), PazError> {
        validate_archive_uri(&self.prefix, uri)?;
        if ![uri, source].into_iter().all(|value| {
            value
                .rsplit_once('.')
                .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("png"))
        }) || self.entry(source).is_err()
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_TEXTURE_VARIANT_SOURCE",
                "texture variant requires an existing archive PNG source",
            ));
        }
        if self.entries.keys().any(|native| {
            let (short, long) = if native.len() < uri.len() {
                (native.as_str(), uri)
            } else {
                (uri, native.as_str())
            };
            long.get(..short.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(short))
                && matches!(long.as_bytes().get(short.len()), None | Some(b'/'))
        }) {
            return Err(error(
                "SHINKI_EMU_MUSICA_TEXTURE_VARIANT_COLLISION",
                "texture variant URI collides with the native archive",
            ));
        }
        Ok(())
    }

    pub(crate) fn texture_source<'a>(&'a self, uri: &'a str) -> &'a str {
        self.texture_variants
            .get(uri)
            .map_or(uri, |variant| variant.source.as_str())
    }

    pub fn read_dir(&self, uri: &str) -> Result<Vec<ArchiveNode>, PazError> {
        validate_archive_directory_uri(&self.prefix, uri)?;
        let base = if uri.ends_with('/') {
            uri.to_owned()
        } else {
            format!("{uri}/")
        };
        let mut children = BTreeMap::new();
        for entry_uri in self
            .entries
            .keys()
            .filter(|candidate| candidate.starts_with(&base))
        {
            let suffix = &entry_uri[base.len()..];
            let Some(name) = suffix.split('/').next() else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let directory = suffix.contains('/');
            children
                .entry(name.to_owned())
                .or_insert_with(|| ArchiveNode {
                    uri: format!("{base}{name}"),
                    name: name.to_owned(),
                    kind: if directory {
                        ArchiveNodeKind::Directory
                    } else {
                        ArchiveNodeKind::File
                    },
                });
        }
        if children.is_empty() && uri != self.prefix {
            return Err(error(
                "SHINKI_EMU_VFS_NOT_FOUND",
                "VFS directory was not found",
            ));
        }
        Ok(children.into_values().collect())
    }

    pub fn stat(&self, uri: &str) -> Result<ArchiveStat, PazError> {
        if uri == self.prefix
            || self
                .entries
                .keys()
                .any(|candidate| candidate.starts_with(&format!("{}/", uri.trim_end_matches('/'))))
        {
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: None,
                kind: ArchiveNodeKind::Directory,
                size: 0,
                archive_role: None,
                method: None,
            });
        }
        let entry = self.entry(uri)?;
        Ok(ArchiveStat {
            uri: uri.into(),
            entry_id: Some(entry.descriptor.entry_id.clone()),
            kind: ArchiveNodeKind::File,
            size: entry.descriptor.unpacked_size,
            archive_role: Some(entry.descriptor.archive_role.clone()),
            method: Some(entry_method(&entry.descriptor).into()),
        })
    }

    pub fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<ArchiveReadResult, PazError> {
        if length > ARCHIVE_MAX_READ_BYTES {
            return Err(error(
                "SHINKI_EMU_VFS_READ_LIMIT",
                "range read exceeds the configured limit",
            ));
        }
        let entry = self.entry(uri)?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| error("SHINKI_EMU_VFS_READ_OVERFLOW", "range read overflowed"))?;
        if offset > entry.descriptor.unpacked_size || end > entry.descriptor.unpacked_size {
            return Err(error(
                "SHINKI_EMU_VFS_READ_BOUNDS",
                "range read is outside the entry",
            ));
        }
        let (decoded, cache_hit) = self.decoded_entry(entry)?;
        let bytes = OwnedByteBuffer::from_owner(
            DecodedRange {
                bytes: decoded,
                start: offset as usize,
                end: end as usize,
            },
            DecodedRange::as_slice,
        );
        Ok(ArchiveReadResult {
            uri: uri.into(),
            offset,
            bytes,
            eof: end == entry.descriptor.unpacked_size,
            cache_hit,
        })
    }

    pub fn open_stream(&self, uri: &str) -> Result<Box<dyn ArchiveStream>, PazError> {
        Ok(Box::new(Cursor::new(
            self.decoded_entry(self.entry(uri)?)?.0,
        )))
    }
}

struct DecodedRange {
    bytes: Vec<u8>,
    start: usize,
    end: usize,
}

impl DecodedRange {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[self.start..self.end]
    }
}

fn validate_role_set(configs: &[PazArchiveConfig]) -> Result<(), PazError> {
    let mut roles = BTreeSet::new();
    for config in configs {
        if !shinki_emu_musica_keys::valid_archive_role(&config.role)
            || !roles.insert(config.role.as_str())
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_ARCHIVE_ROLE",
                "archive role is unknown or duplicated",
            ));
        }
    }
    if configs.is_empty() {
        return Err(error(
            "SHINKI_EMU_MUSICA_ARCHIVE_MISSING",
            "at least one declared PAZ role must be supplied",
        ));
    }
    Ok(())
}

fn parse_archive_index(
    source: &mut ArchiveFile,
    expected_index_xor: u32,
    decrypt_provider: &MusicaPazDecryptProvider,
) -> Result<Vec<ArchiveEntryDescriptor>, PazError> {
    let header = read_source_range(source, 0, if source.version == 0 { 4 } else { 36 })?;
    let layout = format::index_layout(&header, source.version, expected_index_xor, source.length)
        .map_err(format_error)?;
    let (index_offset, encrypted_size) = (layout.offset, layout.size);
    source.xor_key = layout.xor;
    let mut encrypted = read_source_range(source, index_offset, encrypted_size)?;
    source.revision = archive_revision(&source.parts, source.version, source.xor_key, &encrypted);
    xor_byte(&mut encrypted, source.xor_key);
    let decoded = decrypt_bytes(
        decrypt_provider,
        MusicaDecryptDescriptor::Index {
            role: source.role.clone(),
            version: source.version,
            stream_offset: 0,
        },
        &encrypted,
    )?;
    format::parse_index(&decoded, &source.role, source.length, || false).map_err(format_error)
}

fn normalize_entry_name(name: &str) -> Result<String, PazError> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(error(
            "SHINKI_EMU_MUSICA_ENTRY_PATH",
            "PAZ entry name is absolute or traverses",
        ));
    }
    Ok(normalized)
}

fn read_exact_range(
    path: &Path,
    offset: u64,
    length: u64,
    source_length: u64,
) -> Result<Vec<u8>, PazError> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > source_length)
        || length > usize::MAX as u64
    {
        return Err(error(
            "SHINKI_EMU_MUSICA_SOURCE_BOUNDS",
            "archive range is out of bounds",
        ));
    }
    let mut file = File::open(path).map_err(|_| {
        error(
            "SHINKI_EMU_MUSICA_ARCHIVE_OPEN",
            "PAZ archive cannot be opened",
        )
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| error("SHINKI_EMU_MUSICA_ARCHIVE_SEEK", "PAZ archive seek failed"))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes).map_err(|_| {
        error(
            "SHINKI_EMU_MUSICA_ARCHIVE_SHORT_READ",
            "PAZ archive returned a short read",
        )
    })?;
    Ok(bytes)
}

fn read_source_range(source: &ArchiveFile, offset: u64, length: u64) -> Result<Vec<u8>, PazError> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > source.length)
        || length > usize::MAX as u64
    {
        return Err(error(
            "SHINKI_EMU_MUSICA_SOURCE_BOUNDS",
            "archive range is out of bounds",
        ));
    }
    let mut remaining = length;
    let mut logical = 0u64;
    let mut start = offset;
    let mut output = Vec::with_capacity(length as usize);
    for part in &source.parts {
        let part_end = logical + part.length;
        if start >= part_end {
            logical = part_end;
            continue;
        }
        let local = start.saturating_sub(logical);
        let count = remaining.min(part.length - local);
        let bytes = read_exact_range(&part.path, local, count, part.length)?;
        output.extend_from_slice(&bytes);
        remaining -= count;
        start += count;
        logical = part_end;
        if remaining == 0 {
            break;
        }
    }
    if remaining != 0 {
        return Err(error(
            "SHINKI_EMU_MUSICA_ARCHIVE_SHORT_READ",
            "multipart PAZ returned a short read",
        ));
    }
    Ok(output)
}

// The index is already needed for mounting. File metadata supplies the payload
// generation without reading every encrypted asset before the first game frame.
fn archive_revision(parts: &[ArchivePart], version: u8, xor_key: u8, index: &[u8]) -> Hash256 {
    let mut revision = Sha256::new();
    revision.update(b"musica.paz.revision.v1\0");
    revision.update([version, xor_key]);
    revision.update((parts.len() as u64).to_le_bytes());
    for part in parts {
        revision.update(part.length.to_le_bytes());
        match part.modified {
            None => revision.update([0]),
            Some(modified) => {
                let (sign, elapsed) = match modified.duration_since(UNIX_EPOCH) {
                    Ok(elapsed) => (1, elapsed),
                    Err(before) => (2, before.duration()),
                };
                revision.update([sign]);
                revision.update(elapsed.as_nanos().to_le_bytes());
            }
        }
    }
    revision.update(index);
    Hash256::from_bytes(revision.finalize().into())
}

fn validate_entry_ranges(
    source_length: u64,
    entries: &[ArchiveEntryDescriptor],
) -> Result<(), PazError> {
    let mut ranges = Vec::with_capacity(entries.len());
    for entry in entries {
        let end = entry
            .offset
            .checked_add(entry.aligned_size)
            .ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_ENTRY_BOUNDS",
                    "entry encrypted range overflowed",
                )
            })?;
        if end > source_length {
            return Err(error(
                "SHINKI_EMU_MUSICA_ENTRY_BOUNDS",
                "entry encrypted range exceeds the archive",
            ));
        }
        if entry.aligned_size != 0 {
            ranges.push((entry.offset, end));
        }
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[1].0 < pair[0].1) {
        return Err(error(
            "SHINKI_EMU_MUSICA_ENTRY_OVERLAP",
            "PAZ encrypted entry ranges overlap",
        ));
    }
    Ok(())
}

fn verify_source_unchanged(source: &ArchiveFile) -> Result<(), PazError> {
    for part in &source.parts {
        let metadata = std::fs::metadata(&part.path).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SOURCE_CHANGED",
                "archive part disappeared after mount",
            )
        })?;
        if metadata.len() != part.length || metadata.modified().ok() != part.modified {
            return Err(error(
                "SHINKI_EMU_MUSICA_SOURCE_CHANGED",
                "archive part metadata changed after mount",
            ));
        }
    }
    Ok(())
}

fn discover_parts(base: &Path, game_root: &Path) -> Result<Vec<ArchivePart>, PazError> {
    let game_root = game_root.canonicalize().map_err(|_| {
        error(
            "SHINKI_EMU_MUSICA_GAME_ROOT",
            "game root cannot be resolved",
        )
    })?;
    let base = base.canonicalize().map_err(|_| {
        error(
            "SHINKI_EMU_MUSICA_ARCHIVE_OPEN",
            "required PAZ archive cannot be resolved",
        )
    })?;
    if !base.starts_with(&game_root) {
        return Err(error(
            "SHINKI_EMU_MUSICA_ARCHIVE_PATH",
            "PAZ archive resolves outside the game root",
        ));
    }
    let metadata = std::fs::metadata(&base).map_err(|_| {
        error(
            "SHINKI_EMU_MUSICA_ARCHIVE_OPEN",
            "required PAZ archive cannot be opened",
        )
    })?;
    let mut parts = vec![ArchivePart {
        path: base.clone(),
        length: metadata.len(),
        modified: metadata.modified().ok(),
    }];
    let base_text = base.as_os_str().to_string_lossy();
    let mut missing_seen = false;
    for suffix in b'A'..=b'Z' {
        let path = PathBuf::from(format!("{base_text}{}", suffix as char));
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                if missing_seen {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_MULTIPART_GAP",
                        "PAZ multipart suffixes must be contiguous",
                    ));
                }
                if metadata.len() == 0 {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_MULTIPART_EMPTY",
                        "PAZ multipart volume is empty",
                    ));
                }
                let path = path.canonicalize().map_err(|_| {
                    error(
                        "SHINKI_EMU_MUSICA_MULTIPART_OPEN",
                        "PAZ multipart volume cannot be resolved",
                    )
                })?;
                if !path.starts_with(&game_root) {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_ARCHIVE_PATH",
                        "PAZ multipart volume resolves outside the game root",
                    ));
                }
                parts.push(ArchivePart {
                    path,
                    length: metadata.len(),
                    modified: metadata.modified().ok(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing_seen = true,
            Err(_) => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_MULTIPART_OPEN",
                    "PAZ multipart volume cannot be inspected",
                ));
            }
        }
    }
    Ok(parts)
}

fn media_kind(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "sc" => "script",
        "png" | "jpg" | "bmp" => "image",
        "ogg" | "wav" => "audio",
        "mpg" | "avi" | "wmv" => "video",
        _ => "binary",
    }
}

fn entry_method(entry: &ArchiveEntryDescriptor) -> &'static str {
    match (entry.archive_role.as_str(), entry.packed) {
        ("mov", false) => "movie-transform",
        ("mov", true) => "movie-transform+zlib",
        (_, false) => "blowfish+rc4",
        (_, true) => "blowfish+rc4+zlib",
    }
}

fn decrypt_bytes(
    provider: &MusicaPazDecryptProvider,
    descriptor: MusicaDecryptDescriptor,
    bytes: &[u8],
) -> Result<Vec<u8>, PazError> {
    if bytes.is_empty() {
        return Err(error(
            "SHINKI_EMU_MUSICA_DECRYPT_EMPTY",
            "Musica decrypt input is empty",
        ));
    }
    let mut output = Vec::with_capacity(bytes.len());
    for (batch_index, batch) in bytes.chunks(PAZ_DECRYPT_MAX_BATCH_BYTES).enumerate() {
        let batch_offset = u64::try_from(batch_index)
            .ok()
            .and_then(|index| index.checked_mul(PAZ_DECRYPT_MAX_BATCH_BYTES as u64))
            .ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_DECRYPT_OFFSET",
                    "decrypt batch offset overflowed",
                )
            })?;
        let batch_descriptor = descriptor.with_stream_offset(batch_offset);
        let payload = serde_json::to_vec(&batch_descriptor).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_DESCRIPTOR",
                "Musica decrypt descriptor could not be encoded",
            )
        })?;
        let opaque = PazDecryptDescriptor {
            schema_id: MUSICA_DECRYPT_DESCRIPTOR_SCHEMA.into(),
            schema_hash: provider.descriptor_schema_hash(),
            payload,
        };
        let phase = match batch_descriptor {
            MusicaDecryptDescriptor::Index { .. } => PazDecryptPhase::Index,
            MusicaDecryptDescriptor::Entry { .. } => PazDecryptPhase::Entry,
        };
        for (chunk_index, chunk) in batch.chunks(PAZ_DECRYPT_CHUNK_BYTES).enumerate() {
            let chunk_offset = (chunk_index * PAZ_DECRYPT_CHUNK_BYTES) as u64;
            output.extend_from_slice(&provider.decrypt(PazDecryptRequest {
                phase,
                descriptors: std::slice::from_ref(&opaque),
                transport: PazDecryptTransport {
                    chunk_offset,
                    total_size: batch.len() as u64,
                    batch_index: batch_index as u32,
                    input_bound: batch.len() as u64,
                    output_bound: chunk.len() as u64,
                },
                bytes: chunk,
            })?);
        }
    }
    if output.len() != bytes.len() {
        return Err(error(
            "SHINKI_EMU_MUSICA_DECRYPT_SIZE",
            "Musica decrypt output size is inconsistent",
        ));
    }
    Ok(output)
}

impl MusicaDecryptDescriptor {
    fn with_stream_offset(&self, stream_offset: u64) -> Self {
        match self {
            Self::Index { role, version, .. } => Self::Index {
                role: role.clone(),
                version: *version,
                stream_offset,
            },
            Self::Entry { version, entry, .. } => Self::Entry {
                version: *version,
                entry: entry.clone(),
                stream_offset,
            },
        }
    }
}

fn error(code: &'static str, message: impl Into<String>) -> PazError {
    PazError::invalid(code, message)
}

fn format_error(cause: format::FormatError) -> PazError {
    error(cause.code, cause.message)
}

fn cache_error(error_value: PlaintextCacheError) -> PazError {
    match error_value {
        PlaintextCacheError::EntryLimit => error(
            "SHINKI_EMU_MUSICA_CACHE_ENTRY_LIMIT",
            "plaintext cache entry exceeds its configured budget",
        ),
        PlaintextCacheError::Corrupt => error(
            "SHINKI_EMU_MUSICA_CACHE_CORRUPT",
            "plaintext cache metadata or content is corrupt",
        ),
        PlaintextCacheError::Permission(_) => error(
            "SHINKI_EMU_MUSICA_CACHE_PERMISSION",
            "plaintext cache privacy permissions could not be enforced",
        ),
        PlaintextCacheError::Io(_) => {
            error("SHINKI_EMU_MUSICA_CACHE_IO", "plaintext cache I/O failed")
        }
    }
}

#[cfg(test)]
#[path = "paz/tests.rs"]
mod tests;
