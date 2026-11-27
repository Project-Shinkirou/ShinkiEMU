use std::path::{Component, Path, PathBuf};

const MANAGER: &str = "libshinki_emu_manager.so";

fn installed_file(root: &Path, path: &Path) -> Option<PathBuf> {
    // Check the resolved path too: a symlink must not escape the bundle mount.
    let relative = path.strip_prefix(root).ok()?;
    if !relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let resolved = path.canonicalize().ok()?;
    (resolved.starts_with(root) && resolved.is_file()).then_some(resolved)
}

/// An installed Manager image may be an extracted DSO or a member of a HAP.
/// The compiled roster supplies the native basename, never the manifest.
pub(super) fn resolve(
    root: &Path,
    manager: &Path,
    archive_libs: &str,
    basename: &str,
) -> Option<PathBuf> {
    if !basename.starts_with("libshinki_emu_")
        || !basename.ends_with(".so")
        || basename == MANAGER
        || !basename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return None;
    }
    if let Some((hap, member)) = manager.to_str()?.split_once("!/") {
        let hap = Path::new(hap);
        if hap.extension()? != "hap" || member.strip_prefix(archive_libs)? != MANAGER {
            return None;
        }
        let hap = installed_file(root, hap)?;
        if hap.extension()? != "hap" {
            return None;
        }
        return Some(PathBuf::from(format!(
            "{}!/{archive_libs}{basename}",
            hap.to_str()?
        )));
    }
    if manager.file_name()? != MANAGER {
        return None;
    }
    let manager = installed_file(root, manager)?;
    if manager.file_name()? != MANAGER {
        return None;
    }
    let candidate = installed_file(root, &manager.with_file_name(basename))?;
    (candidate.parent() == manager.parent() && candidate.file_name()? == basename)
        .then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ABI_DIR: &str = "libs/arm64-v8a/";
    const COMPANION: &str = "libshinki_emu_fixture.so";

    #[test]
    fn ohos_resolves_only_installed_siblings_and_hap_members() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let base = dir.path().canonicalize()?;
        let bundle = base.join("bundle");
        let app = bundle.join("entry");
        let writable = base.join("filesDir");
        fs::create_dir_all(&app)?;
        fs::create_dir(&writable)?;
        let check = |path: &Path| resolve(&bundle, path, ABI_DIR, COMPANION);
        for folder in [&app, &writable] {
            fs::write(folder.join(MANAGER), [])?;
            fs::write(folder.join(COMPANION), [])?;
            fs::write(folder.join("entry.hap"), [])?;
        }
        let companion = app.join(COMPANION);
        assert_eq!(check(&app.join(MANAGER)), Some(companion.clone()));
        let second = "libshinki_emu_second.so";
        assert_eq!(resolve(&bundle, &app.join(MANAGER), ABI_DIR, second), None);
        fs::write(app.join(second), [])?;
        assert_eq!(
            resolve(&bundle, &app.join(MANAGER), ABI_DIR, second),
            Some(app.join(second))
        );
        assert_eq!(resolve(&bundle, &app.join(MANAGER), ABI_DIR, MANAGER), None);
        assert_eq!(
            resolve(
                &bundle,
                &app.join(MANAGER),
                ABI_DIR,
                "libshinki_emu_a/../b.so"
            ),
            None
        );
        assert_eq!(check(&writable.join(MANAGER)), None);
        let archive_manager = |folder: &Path, libs: &str| {
            PathBuf::from(format!(
                "{}!/{libs}{MANAGER}",
                folder.join("entry.hap").display()
            ))
        };
        assert_eq!(
            check(&archive_manager(&app, ABI_DIR)),
            Some(PathBuf::from(format!(
                "{}!/{ABI_DIR}{COMPANION}",
                app.join("entry.hap").display()
            )))
        );
        assert_eq!(
            resolve(&bundle, &archive_manager(&app, ABI_DIR), ABI_DIR, second),
            Some(PathBuf::from(format!(
                "{}!/{ABI_DIR}{second}",
                app.join("entry.hap").display()
            )))
        );
        assert_eq!(check(&archive_manager(&app, "libs/x86_64/")), None);
        assert_eq!(check(&archive_manager(&writable, ABI_DIR)), None);
        assert_eq!(
            check(&archive_manager(&bundle.join("../filesDir"), ABI_DIR)),
            None
        );
        fs::remove_file(&companion)?;
        assert_eq!(check(&app.join(MANAGER)), None);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(writable.join(COMPANION), &companion)?;
            assert_eq!(check(&app.join(MANAGER)), None);
        }
        Ok(())
    }
}
