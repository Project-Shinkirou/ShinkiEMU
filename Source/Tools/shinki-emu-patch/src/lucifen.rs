use super::Files;
use crate::{PatchError, PatchErrorKind, storage::TaskFile};
use serde::Deserialize;
use shinki_emu_lucifen_formats::{Archive, Bootstrap, Replacements, Sob, member_key};
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArchiveSpec {
    archive: String,
    size: u64,
    sha256: String,
    replacements: Vec<Replacement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    name: String,
    path: String,
}

fn patch_io(error: io::Error) -> PatchError {
    if error
        .get_ref()
        .is_some_and(|error| error.is::<PatchError>())
    {
        return *error
            .into_inner()
            .unwrap()
            .downcast::<PatchError>()
            .unwrap();
    }
    let kind = if error.kind() == io::ErrorKind::InvalidData {
        PatchErrorKind::UnsupportedInput
    } else {
        PatchErrorKind::Io
    };
    PatchError::new(kind, error.to_string())
}

fn read(file: &mut TaskFile, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    file.read(offset, bytes).map_err(io::Error::other)
}

struct StagingWriter<'a>(&'a mut TaskFile, u64);

impl Write for StagingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(self.1, bytes).map_err(io::Error::other)?;
        self.1 += bytes.len() as u64;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn repack(
    files: &Files,
    bootstrap_archive: &str,
    bootstrap_member: &str,
    specs: Vec<ArchiveSpec>,
) -> Result<(), PatchError> {
    let mut seen = BTreeSet::new();
    if specs.is_empty() || !specs.iter().any(|spec| spec.archive == bootstrap_archive) {
        return Err(PatchError::new(
            PatchErrorKind::InvalidRequest,
            "include the bootstrap LPK in the replacement list",
        ));
    }
    // Keys must always come from the original script, even when replacing it.
    let mut input = files.open_input("source", bootstrap_archive)?;
    let bootstrap_spec = specs
        .iter()
        .find(|spec| spec.archive == bootstrap_archive)
        .unwrap();
    let archive = open_original(files, &mut input, bootstrap_spec, &Bootstrap::new())?;
    let entry = archive
        .entry(bootstrap_member)
        .map_err(patch_io)?
        .ok_or_else(|| {
            PatchError::new(
                PatchErrorKind::UnsupportedInput,
                "LPK bootstrap member is missing",
            )
        })?;
    let bootstrap = archive
        .read_member(entry, 16 * 1024 * 1024, |offset, bytes| {
            read(&mut input, offset, bytes)
        })
        .map_err(patch_io)?;
    let keys = Sob::parse(&bootstrap)
        .and_then(|sob| sob.archive_keys())
        .map_err(patch_io)?;

    let mut bootstrap = Some((archive, input));
    for spec in specs {
        let name = member_key(&spec.archive).map_err(patch_io)?;
        if !seen.insert(name) || spec.replacements.is_empty() {
            return Err(PatchError::new(
                PatchErrorKind::InvalidRequest,
                "duplicate LPK or empty replacement list",
            ));
        }
        let (archive, mut input) = if spec.archive == bootstrap_archive {
            bootstrap.take().expect("one bootstrap archive")
        } else {
            let mut input = files.open_input("source", &spec.archive)?;
            let archive = open_original(files, &mut input, &spec, &keys)?;
            (archive, input)
        };
        rewrite(files, &spec, archive, &mut input)?;
    }
    Ok(())
}

fn open_original(
    files: &Files,
    input: &mut TaskFile,
    spec: &ArchiveSpec,
    keys: &Bootstrap,
) -> Result<Archive, PatchError> {
    let size = input.size()?;
    if size != spec.size {
        return Err(release_mismatch(&spec.archive));
    }
    let sha256 = input.sha256()?;
    if sha256 != spec.sha256 {
        return Err(release_mismatch(&spec.archive));
    }
    files.record("source", &spec.archive, sha256)?;
    let key = keys
        .get(&member_key(&spec.archive).map_err(patch_io)?)
        .copied();
    Archive::open(&spec.archive, size, key, |offset, bytes| {
        read(input, offset, bytes)
    })
    .map_err(patch_io)
}

fn release_mismatch(archive: &str) -> PatchError {
    PatchError::new(
        PatchErrorKind::ReleaseMismatch,
        format!("{archive} differs from the source required by this recipe"),
    )
}

fn rewrite(
    files: &Files,
    spec: &ArchiveSpec,
    archive: Archive,
    input: &mut TaskFile,
) -> Result<(), PatchError> {
    let mut replacements = Replacements::default();
    let mut sizes = std::collections::BTreeMap::new();
    for replacement in &spec.replacements {
        if archive
            .entry(&replacement.name)
            .map_err(patch_io)?
            .is_none()
        {
            return Err(PatchError::new(
                PatchErrorKind::UnsupportedInput,
                format!("{} has no member named {}", spec.archive, replacement.name),
            ));
        }
        let size = files.open_input("patch", &replacement.path)?.size()?;
        sizes.insert(replacement.path.as_str(), size);
        replacements
            .insert(&replacement.name, replacement.path.clone(), size)
            .map_err(patch_io)?;
    }

    let output = files.output_handle(&spec.archive, false)?;
    let mut output = output.0.lock();
    let mut current: Option<(String, TaskFile)> = None;
    let mut read_patch = |path: &str, offset, bytes: &mut [u8]| -> Result<(), PatchError> {
        if current.as_ref().is_none_or(|(name, _)| name != path) {
            let mut input = files.open_input("patch", path)?;
            if input.size()? != sizes[path] {
                return Err(PatchError::new(
                    PatchErrorKind::Changed,
                    "replacement length changed during conversion",
                ));
            }
            let hash = input.sha256()?;
            files.record("patch", path, hash)?;
            current = Some((path.to_owned(), input));
        }
        current.as_mut().unwrap().1.read(offset, bytes)
    };
    let mut writer = StagingWriter(&mut output, 0);
    archive
        .rewrite(
            replacements,
            |offset, bytes| read(input, offset, bytes),
            |path, offset, bytes| read_patch(path, offset, bytes).map_err(io::Error::other),
            &mut writer,
        )
        .map_err(patch_io)?;
    let end = writer.1;
    output.truncate(end)?;
    output.close()
}
