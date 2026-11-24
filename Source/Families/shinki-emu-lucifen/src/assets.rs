use shinki_emu_lucifen_formats::{Archive, Bootstrap, SCRIPT_ARCHIVE, Sob, member_key};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const BOOTSTRAP: &str = "gameinit.sob";

struct Mounted {
    file: File,
    index: Archive,
    key: Option<(u32, u32)>,
}

impl Mounted {
    fn open(mut file: File, name: &str, key: Option<(u32, u32)>) -> io::Result<Self> {
        let index = Archive::open(name, file.metadata()?.len(), key, |offset, bytes| {
            read_at(&mut file, offset, bytes)
        })?;
        Ok(Self { index, file, key })
    }

    fn read(&mut self, member: &str, limit: usize) -> io::Result<Option<Vec<u8>>> {
        self.index
            .entry(member)?
            .map(|entry| {
                self.index.read_member(entry, limit, |offset, bytes| {
                    read_at(&mut self.file, offset, bytes)
                })
            })
            .transpose()
    }
}

pub struct GameAssets {
    catalog: BTreeMap<String, PathBuf>,
    declared_keys: Bootstrap,
    archives: BTreeMap<String, Mounted>,
}

impl GameAssets {
    /// Acquire the initial script archive. Other banks use the keys in the
    /// compiled mount calls and are only opened when they are needed.
    pub fn open(root: &Path) -> io::Result<Self> {
        let mut catalog = BTreeMap::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".lpk") {
                continue;
            }
            let name = member_key(&name)?;
            if catalog.insert(name, entry.path()).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ambiguous LPK name",
                ));
            }
        }
        let path = catalog
            .get(SCRIPT_ARCHIVE)
            .ok_or_else(|| missing(SCRIPT_ARCHIVE))?;
        let mut script = Mounted::open(File::open(path)?, SCRIPT_ARCHIVE, None)?;
        let bytes = script
            .read(BOOTSTRAP, 16 * 1024 * 1024)?
            .ok_or_else(|| missing(BOOTSTRAP))?;
        let declared_keys = Sob::parse(&bytes)?.archive_keys()?;
        Ok(Self {
            catalog,
            declared_keys,
            archives: BTreeMap::from([("script".to_owned(), script)]),
        })
    }

    pub fn mount(&mut self, name: &str, key: Option<(u32, u32)>) -> io::Result<()> {
        let name = member_key(name)?;
        let bank = name.strip_suffix(".lpk").ok_or_else(|| missing(&name))?;
        if self
            .archives
            .get(bank)
            .is_some_and(|mounted| mounted.key == key)
        {
            return Ok(());
        }
        let path = self.catalog.get(&name).ok_or_else(|| missing(&name))?;
        let archive = Mounted::open(File::open(path)?, &name, key)?;
        self.archives.insert(bank.to_owned(), archive);
        Ok(())
    }

    pub fn read(&mut self, resource: &str, limit: usize) -> io::Result<Vec<u8>> {
        let resource = member_key(resource)?;
        let (bank, member) = resource.split_once('/').ok_or_else(|| missing(&resource))?;
        if !self.archives.contains_key(bank) {
            let name = format!("{bank}.lpk");
            let key = self.declared_keys.get(&name).copied();
            self.mount(&name, key)?;
        }
        self.archives
            .get_mut(bank)
            .ok_or_else(|| missing(bank))?
            .read(member, limit)?
            .ok_or_else(|| missing(&resource))
    }
}

fn read_at(file: &mut File, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(bytes)
}

fn missing(resource: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("missing Lucifen resource {resource}"),
    )
}
