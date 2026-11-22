//! Mounted read-only data media. Discovery reads OS metadata, not game files.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The selected optical medium is no longer present at its original mount point.
pub const OPTICAL_MEDIA_UNAVAILABLE: &str = "SHINKI_EMU_OPTICAL_MEDIA_UNAVAILABLE: Reinsert the original disc or open the currently mounted disc from the library.";

/// A mounted optical volume as seen when the user opened the disc picker.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct OpticalVolume {
    pub root: PathBuf,
    pub label: Option<String>,
    identity: String,
}

impl OpticalVolume {
    /// Reject empty, unmounted, or replaced media at the selected mount point.
    pub fn validate(&self) -> Result<(), String> {
        self.check(native::at(&self.root).ok().flatten().as_ref())
    }

    /// Opaque OS identity: it may change when the same disc is re-mounted.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    fn check(&self, current: Option<&Self>) -> Result<(), String> {
        if current.is_some_and(|current| current.root == self.root && current.identity == self.identity) {
            Ok(())
        } else {
            Err(OPTICAL_MEDIA_UNAVAILABLE.into())
        }
    }
}

/// List mounted optical data volumes without reading their contents.
pub fn optical_volumes() -> Result<Vec<OpticalVolume>, String> {
    let mut volumes = native::volumes()
        .map_err(|error| format!("SHINKI_EMU_OPTICAL_VOLUMES: {error}"))?;
    volumes.sort_unstable_by(|a, b| a.root.cmp(&b.root));
    volumes.dedup_by(|a, b| a.root == b.root);
    Ok(volumes)
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    use windows::Win32::System::Diagnostics::Debug::{
        GetThreadErrorMode, SEM_FAILCRITICALERRORS, SetThreadErrorMode, THREAD_ERROR_MODE,
    };
    use windows::Win32::System::SystemServices::FILE_READ_ONLY_VOLUME;
    use windows::Win32::System::WindowsProgramming::DRIVE_CDROM;
    use windows::core::PCWSTR;

    pub(super) fn volumes() -> io::Result<Vec<OpticalVolume>> {
        // SAFETY: GetLogicalDrives takes no arguments.
        let drives = unsafe { GetLogicalDrives() };
        if drives == 0 {
            return Err(io::Error::last_os_error());
        }
        let _guard = NoMediaDialog::new()?;
        let mut volumes = Vec::new();
        for index in 0..26 {
            if drives & (1 << index) == 0 {
                continue;
            }
            let letter = b'A' + index;
            let root = PathBuf::from(format!("{}:\\", char::from(letter)));
            if let Some(vol) = volume(root, drive_path(letter)) {
                volumes.push(vol);
            }
        }
        Ok(volumes)
    }

    pub(super) fn at(root: &Path) -> io::Result<Option<OpticalVolume>> {
        let Some(letter) = drive_letter(root) else { return Ok(None); };
        let _guard = NoMediaDialog::new()?;
        Ok(volume(dunce::simplified(root).to_owned(), drive_path(letter)))
    }

    fn drive_letter(root: &Path) -> Option<u8> {
        // Rust canonicalizes drive paths to the verbatim form; Win32 queries
        // require a drive root with its trailing backslash, not `X:`.
        let mut chars = dunce::simplified(root).as_os_str().encode_wide();
        match (chars.next(), chars.next(), chars.next(), chars.next()) {
            (Some(letter @ 65..=90), Some(58), Some(92), None) => Some(letter as u8),
            _ => None,
        }
    }

    fn drive_path(letter: u8) -> [u16; 4] {
        [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0]
    }

    fn volume(root: PathBuf, drive: [u16; 4]) -> Option<OpticalVolume> {
        let drive = PCWSTR(drive.as_ptr());
        let mut label = [0u16; 261];
        let mut fs = [0u16; 16];
        let mut serial = 0;
        let mut flags = 0;
        // SAFETY: drive is NUL-terminated and all the supplied output buffers
        // remain valid for the calls. No device handle is opened.
        unsafe {
            if GetDriveTypeW(drive) != DRIVE_CDROM {
                return None;
            }
            GetVolumeInformationW(
                drive, Some(&mut label), Some(&mut serial), None, Some(&mut flags), Some(&mut fs),
            ).ok()?;
        }
        let fs = wide_field(&fs);
        let label = wide_field(&label);
        // Windows CDFS synthesizes a volume for Red Book audio CDs under this
        // reserved label. Neither it nor an unknown filesystem is game data.
        if flags & FILE_READ_ONLY_VOLUME == 0
            || !matches!(fs.as_str(), "CDFS" | "UDF")
            || fs == "CDFS" && label == "Audio CD"
            || serial == 0
        {
            return None;
        }
        Some(OpticalVolume {
            root,
            identity: format!("win:{fs}:{serial:08x}:{label}"),
            label: (!label.is_empty()).then_some(label),
        })
    }

    fn wide_field(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    struct NoMediaDialog(THREAD_ERROR_MODE);

    impl NoMediaDialog {
        fn new() -> io::Result<Self> {
            // SAFETY: The error mode is thread-local and restored by Drop.
            unsafe {
                let old = THREAD_ERROR_MODE(GetThreadErrorMode());
                SetThreadErrorMode(old | SEM_FAILCRITICALERRORS, None)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                Ok(Self(old))
            }
        }
    }

    impl Drop for NoMediaDialog {
        fn drop(&mut self) {
            // SAFETY: restore the caller's thread-local mode.
            let _ = unsafe { SetThreadErrorMode(self.0, None) };
        }
    }

    #[cfg(test)]
    #[test]
    fn optical_query_accepts_canonical_drive_roots_but_not_shares_or_subdirectories() {
        assert_eq!(drive_letter(Path::new(r"X:\")), Some(b'X'));
        assert_eq!(drive_letter(Path::new(r"\\?\X:\")), Some(b'X'));
        assert_eq!(drive_letter(Path::new(r"X:")), None);
        assert_eq!(drive_letter(Path::new(r"X:\game")), None);
        assert_eq!(drive_letter(Path::new(r"\\server\share\")), None);
    }
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use std::ffi::{CString, OsString};
    use std::fs;
    use std::io;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    pub(super) fn volumes() -> io::Result<Vec<OpticalVolume>> {
        let table = fs::read("/proc/self/mountinfo")?;
        let mut volumes = Vec::new();
        let mut identity_error = None;
        for mount in mounts(&table) {
            match mount.volume() {
                Ok(Some(volume)) => volumes.push(volume),
                Ok(None) => (),
                Err(error) if error.kind() == io::ErrorKind::Unsupported => identity_error = Some(error),
                Err(error) => return Err(error),
            }
        }
        if volumes.is_empty() && let Some(error) = identity_error {
            return Err(error);
        }
        Ok(volumes)
    }

    pub(super) fn at(root: &Path) -> io::Result<Option<OpticalVolume>> {
        let table = fs::read("/proc/self/mountinfo")?;
        for mount in mounts(&table).filter(|mount| mount.root == root) {
            if let Some(volume) = mount.volume()? {
                return Ok(Some(volume));
            }
        }
        Ok(None)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Mount<'a> {
        id: u64,
        device: &'a str,
        root: PathBuf,
    }

    fn mounts(table: &[u8]) -> impl Iterator<Item = Mount<'_>> {
        table.split(|&b| b == b'\n').filter_map(|line| {
            let mut fields = line.split(|&b| b == b' ');
            let id = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
            let _parent = fields.next()?;
            let device = std::str::from_utf8(fields.next()?).ok()?;
            let (major, minor) = device.split_once(':')?;
            major.parse::<u32>().ok()?;
            minor.parse::<u32>().ok()?;
            // Ignore subdirectory bind mounts from an optical volume.
            if fields.next()? != b"/" {
                return None;
            }
            let root = fields.next()?;
            if !fields.next()?.split(|&b| b == b',').any(|option| option == b"ro") {
                return None;
            }
            fields.find(|field| *field == b"-")?;
            if !matches!(fields.next()?, b"iso9660" | b"udf") || !root.starts_with(b"/") {
                return None;
            }
            Some(Mount { id, device, root: PathBuf::from(OsString::from_vec(unescape(root))) })
        })
    }

    fn unescape(field: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(field.len());
        let mut rest = field;
        while let Some((&byte, next)) = rest.split_first() {
            if byte == b'\\' {
                let decoded = match next.get(..3) {
                    Some(b"040") => Some(b' '),
                    Some(b"011") => Some(b'\t'),
                    Some(b"012") => Some(b'\n'),
                    Some(b"134") => Some(b'\\'),
                    _ => None,
                };
                if let Some(value) = decoded {
                    out.push(value);
                    rest = &next[3..];
                    continue;
                }
            }
            out.push(byte);
            rest = next;
        }
        out
    }

    impl Mount<'_> {
        fn volume(self) -> io::Result<Option<OpticalVolume>> {
            let mut device = Path::new("/sys/dev/block").join(self.device);
            if device.join("partition").is_file() {
                let Ok(partition) = fs::canonicalize(&device) else { return Ok(None); };
                let Some(parent) = partition.parent() else { return Ok(None); };
                device = parent.to_owned();
            }
            let optical = fs::read_to_string(device.join("device/type")).is_ok_and(|kind| kind.trim() == "5");
            // Desktop helpers mount ISO/UDF images as kernel loop devices.
            let loop_source = if optical { None } else { fs::read(device.join("loop/backing_file")).ok() };
            if !optical && loop_source.is_none() {
                return Ok(None);
            }
            let sequence = match fs::read_to_string(device.join("diskseq")) {
                Ok(value) if value.trim().parse::<u64>().is_ok() => value,
                _ if !device.exists() => return Ok(None),
                _ => return Err(io::Error::new(io::ErrorKind::Unsupported, "Optical media identity requires Linux disk sequence metadata (Linux 5.15 or newer)")),
            };
            let path = CString::new(self.root.as_os_str().as_bytes())?;
            // SAFETY: path is a live NUL-terminated mount root; access tests
            // directory permissions without opening the block device.
            if unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::X_OK) } != 0 {
                return Ok(None);
            }
            let mut info = std::mem::MaybeUninit::<libc::statx>::uninit();
            // SAFETY: path and statx output storage are valid.
            let info = unsafe {
                if libc::statx(libc::AT_FDCWD, path.as_ptr(), libc::AT_STATX_DONT_SYNC, libc::STATX_MNT_ID, info.as_mut_ptr()) != 0 {
                    return Ok(None);
                }
                info.assume_init()
            };
            if info.stx_mask & libc::STATX_MNT_ID == 0 {
                return Err(io::Error::new(io::ErrorKind::Unsupported, "Cannot verify the active optical mount"));
            }
            // Ignore optical filesystems obscured by another mount.
            if info.stx_mnt_id != self.id {
                return Ok(None);
            }
            let image = loop_source.map(|source| {
                use sha2::{Digest, Sha256};
                hex::encode(Sha256::digest(source))
            }).unwrap_or_default();
            let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
            Ok(Some(OpticalVolume {
                root: self.root,
                label: None,
                // diskseq changes when the kernel observes a media change;
                // unlike a mount ID or /dev/srN, it cannot repeat in this boot.
                identity: format!("linux:{}:{}:{image}", boot.trim(), sequence.trim()),
            }))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn mountinfo_keeps_optical_filesystems_and_decodes_mount_paths() {
            let table = b"\
42 22 11:0 / /media/Game\\040Disc ro,nosuid shared:7 - iso9660 /dev/sr0 ro\n\
43 22 7:0 / /mnt/ISO\\1341 ro - udf /dev/loop0 ro\n\
44 22 11:0 /extras /mnt/alias ro - iso9660 /dev/sr0 ro\n\
45 22 8:2 / /media/Ordinary rw - ext4 /dev/sda2 rw\n\
46 22 0:22 / /media/Share ro - cifs //host/share ro\n\
47 22 0:23 / /media/Audio ro - fuse.gvfsd-fuse gvfsd-fuse ro\n";
            let mounts: Vec<_> = mounts(table).collect();
            assert_eq!(mounts, [
                Mount { id: 42, device: "11:0", root: "/media/Game Disc".into() },
                Mount { id: 43, device: "7:0", root: r"/mnt/ISO\1".into() },
            ]);
        }
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::io;
    use std::mem::{MaybeUninit, size_of};
    use std::os::unix::ffi::OsStrExt;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOBSDNameMatching(main_port: u32, options: u32, name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
        fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
        fn IOObjectRelease(object: u32) -> i32;
    }

    pub(super) fn volumes() -> io::Result<Vec<OpticalVolume>> {
        // SAFETY: NULL with a zero buffer size queries the mount count only.
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut mounts = Vec::<libc::statfs>::with_capacity(count as usize + 8);
        let size = i32::try_from(mounts.capacity() * size_of::<libc::statfs>())
            .map_err(io::Error::other)?;
        // SAFETY: mounts owns the supplied buffer. Only successfully returned
        // entries, capped at the allocated count, are marked initialized.
        let count = unsafe { libc::getfsstat(mounts.as_mut_ptr(), size, libc::MNT_NOWAIT) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe { mounts.set_len((count as usize).min(mounts.capacity())) };
        let mut volumes = Vec::new();
        for mount in &mounts {
            if optical_device(mount).is_some() && let Some(volume) = at(mount_root(mount))? {
                volumes.push(volume);
            }
        }
        Ok(volumes)
    }

    pub(super) fn at(root: &Path) -> io::Result<Option<OpticalVolume>> {
        let path = CString::new(root.as_os_str().as_bytes())?;
        let mut out = MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: path is NUL-terminated and statfs has valid output storage.
        let mount = unsafe {
            if libc::statfs(path.as_ptr(), out.as_mut_ptr()) != 0 {
                return Ok(None);
            }
            out.assume_init()
        };
        if root != mount_root(&mount) {
            return Ok(None);
        }
        let Some(name) = optical_device(&mount) else { return Ok(None); };
        // SAFETY: these calls look up an IOKit registry entry by BSD name.
        // IOServiceGetMatchingService consumes the matching CF dictionary.
        let id = unsafe {
            if libc::access(path.as_ptr(), libc::R_OK | libc::X_OK) != 0 {
                return Ok(None);
            }
            let matching = IOBSDNameMatching(0, 0, name.as_ptr());
            if matching.is_null() {
                return Ok(None);
            }
            let entry = IOServiceGetMatchingService(0, matching);
            if entry == 0 {
                return Ok(None);
            }
            let mut id = 0;
            let result = IORegistryEntryGetRegistryEntryID(entry, &mut id);
            IOObjectRelease(entry);
            if result != 0 || id == 0 {
                return Ok(None);
            }
            id
        };
        let mut boot = [0u8; 64];
        let mut size = boot.len();
        // SAFETY: the boot UUID fits in the output buffer and sysctl validates size.
        let result = unsafe {
            libc::sysctlbyname(c"kern.bootsessionuuid".as_ptr(), boot.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0)
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let boot = CStr::from_bytes_until_nul(&boot).map_err(io::Error::other)?.to_string_lossy();
        Ok(Some(OpticalVolume {
            root: root.to_owned(),
            label: root.file_name().map(|name| name.to_string_lossy().into_owned()),
            identity: format!("mac:{boot}:{id}"),
        }))
    }

    fn mount_root(mount: &libc::statfs) -> &Path {
        Path::new(std::ffi::OsStr::from_bytes(field(&mount.f_mntonname)))
    }

    fn optical_device(mount: &libc::statfs) -> Option<CString> {
        if mount.f_flags & libc::MNT_RDONLY as u32 == 0
            || !matches!(field(&mount.f_fstypename), b"cd9660" | b"udf")
        {
            return None;
        }
        let bsd_name = field(&mount.f_mntfromname).strip_prefix(b"/dev/")?;
        if !bsd_name.starts_with(b"disk") {
            return None;
        }
        CString::new(bsd_name).ok()
    }

    fn field(buffer: &[c_char]) -> &[u8] {
        // SAFETY: c_char and u8 have the same size and alignment; mount fields
        // are byte strings. Search for NUL bounded by the actual field length.
        let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), buffer.len()) };
        bytes.split(|&byte| byte == 0).next().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_media_does_not_follow_a_reused_mount_path() {
        let selected = OpticalVolume { root: "disc".into(), label: Some("DEMO".into()), identity: "first".into() };
        let replacement = OpticalVolume { identity: "second".into(), ..selected.clone() };
        assert!(selected.check(None).is_err());
        assert!(selected.check(Some(&replacement)).is_err());
        assert!(selected.check(Some(&selected)).is_ok());
        assert!(replacement.check(Some(&replacement)).is_ok());
    }
}
