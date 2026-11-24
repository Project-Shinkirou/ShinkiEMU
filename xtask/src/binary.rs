use anyhow::{Context, Result, bail};
use object::{
    read::{
        elf::{Dyn, FileHeader, ProgramHeader},
        macho::{FatArch, MachHeader, MachOFatFile, MachOFatFile32, MachOFatFile64},
        pe::ImageNtHeaders,
    },
};
use shinki_emu_build::ohos::Target as OhosTarget;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub(crate) const ANDROID_API: u32 = 28;
fn text(bytes: &[u8]) -> Result<String> {
    Ok(std::str::from_utf8(bytes)?.to_owned())
}
fn pe<'a, P: ImageNtHeaders>(
    file: &object::read::pe::PeFile<'a, P>,
    dependencies: &mut BTreeSet<String>,
) -> Result<()> {
    if let Some(table) = file.import_table()? {
        let mut it = table.descriptors()?;
        while let Some(d) = it.next()? {
            dependencies
                .insert(text(table.name(d.name.get(object::LittleEndian))?)?);
        }
    }
    if let Some(table) = file
        .data_directories()
        .delay_load_import_table(file.data(), &file.section_table())?
    {
        let mut it = table.descriptors()?;
        while let Some(d) = it.next()? {
            dependencies
                .insert(text(table.name(d.dll_name_rva.get(object::LittleEndian))?)?);
        }
    }
    Ok(())
}
fn elf<'a, E: FileHeader>(
    file: &object::read::elf::ElfFile<'a, E>,
    dependencies: &mut BTreeSet<String>,
) -> Result<()> {
    let endian = file.endian();
    for segment in file.elf_program_headers() {
        let Some(entries) = segment.dynamic(endian, file.data())? else {
            continue;
        };
        let entries = &entries[..entries
            .iter()
            .position(|entry| entry.tag(endian) == object::elf::DT_NULL)
            .context("ELF dynamic segment lacks DT_NULL")?];
        if !entries.iter().any(|entry| entry.tag(endian) == object::elf::DT_NEEDED) {
            continue;
        }
        let address: u64 = entries
            .iter()
            .find(|entry| entry.tag(endian) == object::elf::DT_STRTAB)
            .context("ELF dynamic string table address missing")?
            .d_val(endian)
            .into();
        let size: u64 = entries
            .iter()
            .find(|entry| entry.tag(endian) == object::elf::DT_STRSZ)
            .context("ELF dynamic string table size missing")?
            .d_val(endian)
            .into();
        let mut data = None;
        for load in file.elf_program_headers() {
            if load.p_type(endian) != object::elf::PT_LOAD {
                continue;
            }
            data = load
                .data_range(endian, file.data(), address, size)
                .map_err(|_| anyhow::anyhow!("invalid ELF load segment range"))?;
            if data.is_some() {
                break;
            }
        }
        let data = data.context("ELF dynamic strings are not in a loadable segment")?;
        let strings = object::read::StringTable::new(data, 0, size);
        for entry in entries {
            if entry.tag(endian) == object::elf::DT_NEEDED {
                dependencies.insert(text(entry.string(endian, strings)?)?);
            }
        }
    }
    Ok(())
}

fn macho<'a, M: MachHeader>(
    file: &object::read::macho::MachOFile<'a, M>,
    dependencies: &mut BTreeSet<String>,
) -> Result<()> {
    let e = file.endian();
    let mut it = file.macho_header().load_commands(e, file.data(), 0)?;
    while let Some(c) = it.next()? {
        if c.cmd() != object::macho::LC_ID_DYLIB
            && let Some(d) = c.dylib()?
        {
            dependencies.insert(text(c.string(e, d.dylib.name)?)?);
        }
    }
    Ok(())
}
fn fat_slice<'a, A: FatArch>(
    file: &MachOFatFile<'a, A>,
    bytes: &'a [u8],
    architecture: object::Architecture,
) -> Result<&'a [u8]> {
    let mut matches = file
        .arches()
        .iter()
        .filter(|arch| arch.architecture() == architecture);
    let arch = matches
        .next()
        .context("universal binary lacks the requested architecture")?;
    if matches.next().is_some() {
        bail!("universal binary repeats the requested architecture");
    }
    Ok(arch.data(bytes)?)
}

pub(crate) fn target_slice(bytes: &[u8], architecture: object::Architecture) -> Result<&[u8]> {
    match object::FileKind::parse(bytes)? {
        object::FileKind::MachOFat32 => {
            fat_slice(&MachOFatFile32::parse(bytes)?, bytes, architecture)
        }
        object::FileKind::MachOFat64 => {
            fat_slice(&MachOFatFile64::parse(bytes)?, bytes, architecture)
        }
        _ => Ok(bytes),
    }
}

pub(crate) fn dependencies(path: &Path, target: &str) -> Result<BTreeSet<String>> {
    crate::package::plain(path)?;
    let bytes = fs::read(path)?;
    let ohos = OhosTarget::from_rust_target(target);
    let arch = if let Some(target) = ohos {
        target.architecture()
    } else if target.starts_with("aarch64-") {
        object::Architecture::Aarch64
    } else {
        object::Architecture::X86_64
    };
    let file = object::File::parse(target_slice(&bytes, arch)?)
        .with_context(|| format!("reading dependencies of {}", path.display()))?;
    if ohos.is_some()
        && (!matches!(file, object::File::Elf64(_)) || object::Object::architecture(&file) != arch)
    {
        bail!("{} is not a {target} ELF64 library", path.display());
    }
    let mut names = BTreeSet::new();
    match &file {
        object::File::Pe32(f) => pe(f, &mut names)?,
        object::File::Pe64(f) => pe(f, &mut names)?,
        object::File::Elf32(f) => elf(f, &mut names)?,
        object::File::Elf64(f) => elf(f, &mut names)?,
        object::File::MachO32(f) => macho(f, &mut names)?,
        object::File::MachO64(f) => macho(f, &mut names)?,
        _ => bail!("unsupported binary format: {}", path.display()),
    }
    Ok(names)
}

pub(crate) fn system(name: &str, target: &str) -> bool {
    if target.contains("windows") {
        let n = name.to_ascii_lowercase();
        return n.starts_with("api-ms-win-")
            || n.starts_with("ext-ms-win-")
            || [
                "kernel32.dll",
                "kernelbase.dll",
                "user32.dll",
                "gdi32.dll",
                "gdiplus.dll",
                "gdi32full.dll",
                "advapi32.dll",
                "shell32.dll",
                "ole32.dll",
                "oleaut32.dll",
                "combase.dll",
                "uuid.dll",
                "ntdll.dll",
                "ws2_32.dll",
                "bcrypt.dll",
                "bcryptprimitives.dll",
                "crypt32.dll",
                "secur32.dll",
                "rpcrt4.dll",
                "shlwapi.dll",
                "winmm.dll",
                "version.dll",
                "imm32.dll",
                "setupapi.dll",
                "cfgmgr32.dll",
                "dwmapi.dll",
                "uxtheme.dll",
                "d3d9.dll",
                "d3d11.dll",
                "d3d12.dll",
                "dxgi.dll",
                "dwrite.dll",
                "d2d1.dll",
                "d3dcompiler_47.dll",
                "opengl32.dll",
                "glu32.dll",
                "winspool.drv",
                "msvcrt.dll",
                "ucrtbase.dll",
                "powrprof.dll",
                "hid.dll",
                "avrt.dll",
                "mmdevapi.dll",
                "avicap32.dll",
                "propsys.dll",
                "mf.dll",
                "mfplat.dll",
                "mfreadwrite.dll",
                "mfuuid.dll",
                "winhttp.dll",
                "wininet.dll",
                "normaliz.dll",
                "iphlpapi.dll",
                "dnsapi.dll",
                "userenv.dll",
                "psapi.dll",
                "dbghelp.dll",
                "dbgcore.dll",
                "msimg32.dll",
                "comdlg32.dll",
                "comctl32.dll",
                "uiautomationcore.dll",
                "wtsapi32.dll",
                "netapi32.dll",
                "wintrust.dll",
                "imagehlp.dll",
                "dhcpcsvc.dll",
                "ncrypt.dll",
                "usp10.dll",
                "winusb.dll",
                "xinput1_4.dll",
                "dsound.dll",
                "msacm32.dll",
                "authz.dll",
                "wldap32.dll",
                "sensapi.dll",
            ]
            .contains(&n.as_str());
    }
    if target.contains("android") {
        return android_system(name);
    }
    if target.ends_with("ohos") {
        // OpenHarmony NDK system libraries are supplied by the device image.
        return [
            "libc.so",
            "libace_napi.z.so",
            "libace_ndk.z.so",
            "libnative_window.so",
            "libnative_buffer.so",
            "libnative_vsync.so",
            "libohaudio.so",
            "libhilog_ndk.z.so",
            // Pulled by iana-time-zone's OHOS backend (chrono local time).
            "libtime_service_ndk.so",
            "librawfile.z.so",
            "libEGL.so",
            "libGLESv3.so",
            "libz.so",
        ]
        .contains(&name);
    }
    if target.contains("apple") {
        return name.starts_with("/usr/lib/") || name.starts_with("/System/Library/Frameworks/");
    }
    [
        "libc.so.6",
        "libm.so.6",
        "libdl.so.2",
        "libpthread.so.0",
        "librt.so.1",
        "libutil.so.1",
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
        "libgcc_s.so.1",
        // Linux desktop ABI supplied by the supported distribution, not bundled
        // from the developer machine. Keep versioned SONAMEs explicit.
        "libstdc++.so.6",
        "libgomp.so.1",
        "libfontconfig.so.1",
        "libudev.so.1",
        "libasound.so.2",
        // VA-API talks to the distribution's DRM device/driver ABI.
        "libdrm.so.2",
        // OpenGL dispatch belongs to the distribution's graphics driver stack.
        "libGL.so.1",
        "libGLX.so.0",
        "libOpenGL.so.0",
        "libX11.so.6",
        // RFVP's Linux video-sys backend uses the distribution media stack.
        "libgstreamer-1.0.so.0",
        "libgstapp-1.0.so.0",
        "libgstbase-1.0.so.0",
        "libgstvideo-1.0.so.0",
        "libglib-2.0.so.0",
        "libgobject-2.0.so.0",
        "libgio-2.0.so.0",
        "libgmodule-2.0.so.0",
        // Manager store sign-in uses the distribution's WebKitGTK 4.1 / GTK 3.
        "libwebkit2gtk-4.1.so.0",
        "libjavascriptcoregtk-4.1.so.0",
        "libsoup-3.0.so.0",
        "libgtk-3.so.0",
        "libgdk-3.so.0",
        "libatk-1.0.so.0",
        "libcairo.so.2",
        "libcairo-gobject.so.2",
        "libpango-1.0.so.0",
        "libpangocairo-1.0.so.0",
        "libpangoft2-1.0.so.0",
        "libgdk_pixbuf-2.0.so.0",
        "libharfbuzz.so.0",
    ]
    .contains(&name)
}

// Public NDK libraries supported by the Android app's minimum API (28).
// Bionic's names are case-sensitive even when packaging on Windows.
fn android_system(name: &str) -> bool {
    [
        "libc.so",
        "libm.so",
        "libdl.so",
        "liblog.so",
        "libandroid.so",
        "libEGL.so",
        "libGLESv1_CM.so",
        "libGLESv2.so",
        "libGLESv3.so",
        "libvulkan.so",
        "libaaudio.so",
        "libcamera2ndk.so",
        "libOpenSLES.so",
        "libmediandk.so",
        "libnativewindow.so",
        "libz.so",
        "libjnigraphics.so",
        "libsync.so",
    ]
    .contains(&name)
}
/// Resolve Linux SONAME links only within their explicitly selected build directory.
/// Package entries keep the requested loader name but copy the regular file's bytes.
pub(crate) fn runtime_source(path: &Path, directory: &Path, target: &str) -> Result<PathBuf> {
    crate::package::plain(directory)?;
    if target == "x86_64-unknown-linux-gnu" && fs::symlink_metadata(path)?.is_symlink() {
        let root = fs::canonicalize(directory)?;
        let source = fs::canonicalize(path)?;
        if !source.starts_with(&root) || !source.is_file() {
            bail!(
                "runtime link escapes its build directory: {}",
                path.display()
            );
        }
        crate::package::plain(&source)?;
        return Ok(source);
    }
    crate::package::plain(path)?;
    Ok(path.to_owned())
}

/// Resolve exclusively against explicitly distributable trees, never PATH/System32.
pub fn closure(library: &Path, search: &[PathBuf], target: &str) -> Result<Vec<(PathBuf, String)>> {
    let windows = target.contains("windows");
    let key = |name: &str| {
        if windows { name.to_ascii_lowercase() } else { name.to_owned() }
    };
    let mut found = BTreeMap::<String, (PathBuf, String)>::new();
    let mut visited = BTreeSet::new();
    let mut pending = vec![library.to_owned()];
    let root_name = key(&library.file_name().context("library filename")?.to_string_lossy());
    while let Some(path) = pending.pop() {
        let name = key(&path.file_name().context("binary filename")?.to_string_lossy());
        if !visited.insert(name) {
            continue;
        }
        for dep in dependencies(&path, target)? {
            if system(&dep, target) {
                continue;
            }
            let leaf = dep.rsplit('/').next().context("dependency filename")?;
            crate::package::portable(leaf)?;
            let dependency_key = key(leaf);
            if dependency_key == root_name || found.contains_key(&dependency_key) {
                continue;
            }
            let mut resolved: Option<PathBuf> = None;
            for dir in search.iter().filter(|dir| dir.is_dir()) {
                let candidate = dir.join(leaf);
                let candidate = if fs::symlink_metadata(&candidate).is_ok() {
                    candidate
                } else if windows && !cfg!(windows) {
                    let mut entry = None;
                    for item in fs::read_dir(dir)? {
                        let item = item?;
                        if item.file_name().to_str().is_some_and(|name| name.eq_ignore_ascii_case(leaf)) {
                            entry = Some(item.path());
                            break;
                        }
                    }
                    let Some(entry) = entry else { continue };
                    entry
                } else {
                    continue;
                };
                let candidate = runtime_source(&candidate, dir, target)?;
                match resolved.as_ref() {
                    Some(path) if path != &candidate => {
                        if crate::package::sha256(path)? != crate::package::sha256(&candidate)? {
                            bail!("conflicting dependency {dep}");
                        }
                    }
                    Some(_) => {}
                    None => resolved = Some(candidate),
                }
            }
            let resolved = resolved.with_context(|| format!(
                "{}: missing redistributable dependency {dep}", path.display()
            ))?;
            pending.push(resolved.clone());
            found.insert(dependency_key, (resolved, leaf.to_owned()));
        }
    }
    Ok(found.into_values().collect())
}
