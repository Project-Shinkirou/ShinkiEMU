//! Ordered composition of read-only recognition rules built into the Manager.
use super::siglus::SelectedRoot;
use super::*;

pub(super) fn recognize<T: DetectionTree>(
    scan: &mut Scan<'_, T>,
    root: &Path,
) -> DetectionResult<()> {
    let mut advdata = adv32::DataDirectory::default();
    let directory = Directory::read(scan, root, Path::new(""), |name| advdata.observe(name))?;
    let siglus = SelectedRoot::select(scan, root, &directory.files)?;
    directory.inspect_files(scan, root, &siglus)?;

    scan.musica(root, &directory.files)?;
    scan.loose_krkr(root, &directory.files)?;
    scan.softpal_manifest(root, |scan, path| inspect_file(scan, root, path, &siglus))?;
    // An append already covered by lookahead keeps its existing scan position.
    if let Some(path) = siglus.append_directory().filter(|path| !is_lookahead(path)) {
        inspect_directory(scan, root, path, &siglus)?;
    }
    advdata.inspect(scan, root)?;

    directory.inspect_children(scan, root, &siglus)
}

fn lookahead_child(parent: &Path, name: &OsStr) -> bool {
    if parent.as_os_str().is_empty() {
        ["data", "startdata", "game", "usrdir"]
            .into_iter()
            .any(|child| name.eq_ignore_ascii_case(child))
    } else {
        parent.as_os_str().eq_ignore_ascii_case("data") && name.eq_ignore_ascii_case("pack")
    }
}

fn is_lookahead(path: &Path) -> bool {
    path.parent()
        .zip(path.file_name())
        .is_some_and(|(parent, name)| lookahead_child(parent, name))
}

struct Directory {
    files: Vec<PathBuf>,
    children: Vec<PathBuf>,
}

impl Directory {
    fn read<T: DetectionTree>(
        scan: &mut Scan<'_, T>,
        root: &Path,
        relative: &Path,
        mut other_directory: impl FnMut(OsString),
    ) -> DetectionResult<Self> {
        scan.checkpoint()?;
        let mut files = Vec::new();
        let mut children = Vec::new();
        scan.entries(&root.join(relative), |name, kind| {
            if kind == EntryKind::File {
                files.push(relative.join(name));
            } else if kind == EntryKind::Directory {
                if lookahead_child(relative, &name) {
                    children.push(relative.join(name));
                } else {
                    other_directory(name);
                }
            }
            Ok(())
        })?;
        files.sort();
        children.sort();
        Ok(Self { files, children })
    }

    fn inspect_files<T: DetectionTree>(
        &self,
        scan: &mut Scan<'_, T>,
        root: &Path,
        siglus: &SelectedRoot,
    ) -> DetectionResult<()> {
        for path in &self.files {
            scan.checkpoint()?;
            inspect_file(scan, root, path, siglus)?;
        }
        Ok(())
    }

    fn inspect_children<T: DetectionTree>(
        self,
        scan: &mut Scan<'_, T>,
        root: &Path,
        siglus: &SelectedRoot,
    ) -> DetectionResult<()> {
        for path in self.children {
            inspect_directory(scan, root, &path, siglus)?;
        }
        Ok(())
    }
}

fn inspect_directory<T: DetectionTree>(
    scan: &mut Scan<'_, T>,
    root: &Path,
    path: &Path,
    siglus: &SelectedRoot,
) -> DetectionResult<()> {
    let directory = Directory::read(scan, root, path, |_| {})?;
    directory.inspect_files(scan, root, siglus)?;
    directory.inspect_children(scan, root, siglus)
}

fn inspect_file<T: DetectionTree>(
    scan: &mut Scan<'_, T>,
    root: &Path,
    path: &Path,
    siglus: &SelectedRoot,
) -> DetectionResult<()> {
    scan.count_file()?;
    scan.checkpoint()?;
    let full = root.join(path);
    let Some(mut source) = scan.open(&full, 0)? else {
        return Ok(());
    };
    let mut head = [0; 128];
    let size = source.length.min(head.len() as u64) as usize;
    scan.read_at(&mut source, 0, &mut head[..size])?;
    match scan.formats(path, &mut source, &head[..size], siglus) {
        Ok(()) | Err(formats::Rejected::NoMatch) => Ok(()),
        Err(formats::Rejected::Abort(error)) => Err(error),
    }
}
