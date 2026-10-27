mod access;
mod custom_glyph;
mod mount;
pub(crate) mod native;
pub(crate) mod save_catalog;
mod settings;
pub(crate) use settings::DEFAULT_FONT as DEFAULT_NATIVE_FONT;
mod source;
mod storage;
mod system_state;
mod window_icon;
use source::*;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Mutex,
    time::SystemTime,
};

use roukaku_byte_source::OwnedByteBuffer;
use shinki_emu_sdk::{
    ARCHIVE_MAX_READ_BYTES, ArchiveNodeKind, ArchiveReadResult, ArchiveStat, CoreError,
    validate_archive_uri,
};

use crate::{
    CmvsCpzHeader, CmvsCpzVersion, CmvsSchemeProfile, Cpz5IndexEntry, PbDecodedImage,
    PbImageResolver, decode_pb2_image, decode_pb3_jbp, decode_pb3_type1, decode_pb3_type5,
    decode_pb3_type6, decrypt_cpz5_entry, parse_cpz_header, parse_cpz5_index, parse_pb2_metadata,
    parse_pb3_metadata,
};

const MAX_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;
/// Decoded entries below this size stay resident as the single most recent
/// entry, so repeated range reads of one script or image decrypt once.
const EPHEMERAL_ENTRY_LIMIT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVES: usize = 256;
const MAX_ENTRIES: usize = 2_000_000;
const MAX_PB3_IMAGE_CHAIN: usize = 32;

#[derive(Clone)]
struct ArchiveFile {
    role: String,
    path: PathBuf,
    source_stamp: SourceStamp,
    header: CmvsCpzHeader,
}

#[derive(Clone)]
struct LooseSource {
    path: PathBuf,
    byte_size: u64,
    source_stamp: SourceStamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceStamp {
    byte_size: u64,
    modified: SystemTime,
}

#[derive(Clone)]
struct MountedEntry {
    ordinal: usize,
    uri: String,
    entry_id: String,
    archive: usize,
    descriptor: Cpz5IndexEntry,
}

pub struct CmvsArchive {
    game_root: Option<PathBuf>,
    prefix: String,
    archives: Vec<ArchiveFile>,
    entries: BTreeMap<String, MountedEntry>,
    loose: BTreeMap<String, LooseSource>,
    scheme: CmvsSchemeProfile,
    ephemeral_entry: Mutex<Option<(String, OwnedByteBuffer)>>,
}

struct CmvsPbImageResolver<'a> {
    vfs: &'a CmvsArchive,
    parent_uri: &'a str,
    chain: Vec<String>,
}

impl PbImageResolver for CmvsPbImageResolver<'_> {
    fn resolve_pb3_base(&self, reference: &str) -> Result<PbDecodedImage, CoreError> {
        let uri = resolve_pb3_base_uri(&self.vfs.prefix, self.parent_uri, reference)?;
        self.vfs.decode_pb3_image(&uri, self.chain.clone())
    }
}

impl CmvsArchive {
    fn entry(&self, uri: &str) -> Result<&MountedEntry, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        self.entries
            .get(uri)
            .ok_or_else(|| invalid("SHINKI_EMU_VFS_NOT_FOUND", "VFS entry was not found"))
    }

    fn loose_role_from_uri(&self, uri: &str) -> Option<String> {
        let loose_prefix = format!("{}loose/", self.prefix);
        let role = uri.strip_prefix(&loose_prefix)?;
        if role.is_empty() || role.contains('/') {
            return None;
        }
        self.loose.contains_key(role).then(|| role.to_owned())
    }

    fn read_loose_range(
        &self,
        source: &LooseSource,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, CoreError> {
        verify_loose_stamp(source)?;
        let mut file = File::open(&source.path).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_LOOSE_IO",
                "CMVS loose resource could not be opened",
            )
        })?;
        file.seek(SeekFrom::Start(offset)).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_LOOSE_IO",
                "CMVS loose resource seek failed",
            )
        })?;
        let length = usize::try_from(length).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_LOOSE_IO",
                "CMVS loose resource range exceeds platform bounds",
            )
        })?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_LOOSE_IO",
                "CMVS loose resource was truncated",
            )
        })?;
        verify_loose_stamp(source)?;
        Ok(bytes)
    }

    /// Decodes a PB2 or PB3 image through this mounted session.  PB3 type-6
    /// keeps its base-image resolution inside the host-owned VFS; PB2 has no
    /// documented cross-entry dependency in the supported reference variants.
    pub fn decode_pb_image(&self, uri: &str) -> Result<PbDecodedImage, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        let source = self.resource_bytes(uri)?;
        if source.starts_with(crate::PB2_MAGIC) {
            return decode_pb2_image(&source);
        }
        self.decode_pb3_image(uri, Vec::new())
    }

    pub(crate) fn native_image_info(&self, uri: &str) -> Result<[u32; 3], CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        let source = self.resource_bytes(uri)?;
        let metadata = if source.starts_with(crate::PB2_MAGIC) {
            parse_pb2_metadata(&source)?
        } else {
            parse_pb3_metadata(&source)?
        };
        Ok([
            u32::from(metadata.width),
            u32::from(metadata.height),
            u32::from(metadata.bits_per_pixel == 32),
        ])
    }

    pub(crate) fn load_cmv(&self, uri: &str) -> Result<crate::cmv::CmvDecoder, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        let source = self.resource_bytes(uri)?;
        crate::cmv::CmvDecoder::new(source)
    }

    /// Base-image lookup stays here rather than in the format parser so
    /// archive identity, URI safety, recursion depth, and cycle detection
    /// remain host-owned.
    fn decode_pb3_image(
        &self,
        uri: &str,
        mut chain: Vec<String>,
    ) -> Result<PbDecodedImage, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        if chain.len() >= MAX_PB3_IMAGE_CHAIN || chain.iter().any(|candidate| candidate == uri) {
            return Err(invalid(
                "SHINKI_EMU_CMVS_PB3_BASE_CYCLE",
                "CMVS PB3 base-image dependency is cyclic or exceeds its depth budget",
            ));
        }
        let source = self.resource_bytes(uri)?;
        let metadata = parse_pb3_metadata(&source)?;
        chain.push(uri.into());
        match metadata.image_type {
            1 => decode_pb3_type1(&source),
            2 | 3 => decode_pb3_jbp(&source),
            5 => decode_pb3_type5(&source),
            6 => decode_pb3_type6(
                &source,
                &CmvsPbImageResolver {
                    vfs: self,
                    parent_uri: uri,
                    chain,
                },
            ),
            _ => Err(invalid(
                "SHINKI_EMU_CMVS_PB3_VARIANT",
                "CMVS PB3 image variant is not implemented",
            )),
        }
    }

    fn decode_entry(&self, entry: &MountedEntry) -> Result<(OwnedByteBuffer, bool), CoreError> {
        let archive = &self.archives[entry.archive];
        verify_archive_stamp(archive)?;
        let ephemeral = entry.descriptor.stored_size < EPHEMERAL_ENTRY_LIMIT_BYTES;
        if ephemeral {
            let resident = self.ephemeral_entry.lock().map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_EPHEMERAL_CACHE",
                    "CMVS ephemeral entry cache is poisoned",
                )
            })?;
            if let Some((entry_id, bytes)) = resident.as_ref()
                && entry_id == &entry.entry_id
            {
                return Ok((bytes.clone(), true));
            }
        }
        let mut source = File::open(&archive.path).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_SOURCE_IO",
                "CMVS archive source could not be opened",
            )
        })?;
        source
            .seek(SeekFrom::Start(entry.descriptor.offset))
            .map_err(|_| invalid("SHINKI_EMU_CMVS_SOURCE_IO", "CMVS entry seek failed"))?;
        let length = usize::try_from(entry.descriptor.stored_size).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_ENTRY_SIZE",
                "CMVS entry exceeds platform bounds",
            )
        })?;
        let mut bytes = vec![0; length];
        source.read_exact(&mut bytes).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_SOURCE_SHORT_READ",
                "CMVS entry source was truncated",
            )
        })?;
        verify_archive_stamp(archive)?;
        decrypt_cpz5_entry(&archive.header, &entry.descriptor, &mut bytes, &self.scheme)?;
        let bytes = OwnedByteBuffer::from(bytes);
        if ephemeral {
            let mut resident = self.ephemeral_entry.lock().map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_EPHEMERAL_CACHE",
                    "CMVS ephemeral entry cache is poisoned",
                )
            })?;
            *resident = Some((entry.entry_id.clone(), bytes.clone()));
        }
        Ok((bytes, false))
    }
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

pub(crate) fn resolve_pb3_base_uri(
    prefix: &str,
    parent_uri: &str,
    reference: &str,
) -> Result<String, CoreError> {
    if reference.is_empty()
        || reference.contains(['/', '\\'])
        || reference.contains("..")
        || !parent_uri.starts_with(prefix)
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_PB3_BASE_REFERENCE",
            "CMVS PB3 base image reference is invalid",
        ));
    }
    let (directory, _) = parent_uri.rsplit_once('/').ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_PB3_BASE_REFERENCE",
            "CMVS PB3 parent URI has no filename",
        )
    })?;
    let uri = format!("{directory}/{reference}");
    validate_archive_uri(prefix, &uri)?;
    Ok(uri)
}

#[cfg(test)]
#[path = "archive/cache_tests.rs"]
pub(crate) mod cache_tests;

#[cfg(test)]
pub(crate) use cache_tests::fixture as test_archive;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_pb3_base_in_the_current_vfs_directory() {
        assert_eq!(
            resolve_pb3_base_uri("cmvs:/", "cmvs:/scene/current.pb3", "base.pb3").unwrap(),
            "cmvs:/scene/base.pb3"
        );
        assert_eq!(
            resolve_pb3_base_uri("cmvs:/", "cmvs:/scene/current.pb3", "../base.pb3")
                .unwrap_err()
                .code(),
            "SHINKI_EMU_CMVS_PB3_BASE_REFERENCE"
        );
    }
}
