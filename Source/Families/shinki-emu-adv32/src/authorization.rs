use crate::{Error, Result};
use std::path::Path;

#[cfg(windows)]
pub(crate) fn check_disc(data: &Path) -> Result<()> {
    use std::ptr;
    use windows_sys::Win32::{
        Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW},
        System::{
            Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetThreadErrorMode},
            WindowsProgramming::DRIVE_CDROM,
        },
    };

    let root = data
        .parent()
        .ok_or(Error::Invalid("ADV32 game directory"))?;
    let expected = crate::native_data::disc_volume_name(root)?
        .ok_or(Error::Unsupported("ADV32 original optical volume metadata"))?;
    let mut previous = 0;
    // An empty drive must not open a Windows media dialog on the core worker.
    if unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut previous) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mounted = (b'A'..=b'Z').any(|letter| {
        let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
        let mut volume = [0u16; 261];
        // Both arrays remain valid for the call. The other outputs are optional.
        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_CDROM {
            return false;
        }
        let read = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                volume.as_mut_ptr(),
                volume.len() as u32,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            )
        };
        let end = volume
            .iter()
            .position(|&code| code == 0)
            .unwrap_or(volume.len());
        read != 0 && String::from_utf16_lossy(&volume[..end]).eq_ignore_ascii_case(&expected)
    });
    unsafe { SetThreadErrorMode(previous, ptr::null_mut()) };
    if mounted {
        Ok(())
    } else {
        Err(Error::DiscRequired)
    }
}

#[cfg(not(windows))]
pub(crate) fn check_disc(_: &Path) -> Result<()> {
    Err(Error::Unsupported(
        "ADV32 optical media detection on this host",
    ))
}
