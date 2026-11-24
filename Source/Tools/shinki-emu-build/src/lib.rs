//! Host-side native build support for ShinkiEMU and its adapters.

pub mod inputs;
pub mod vcpkg;

use anyhow::{Context, Result, bail};
use object::Object;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const ABI: &str = "shinki.emu.native.c.v3";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildAsset {
    pub source: PathBuf,
    pub dest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreBuildManifest {
    pub plugin_id: String,
    /// Cargo target used by adapter build scripts when selecting packaged assets.
    pub target: String,
    pub artifact_kind: String,
    pub native_library: Option<PathBuf>,
    pub resources: Vec<BuildAsset>,
    pub licenses: Vec<BuildAsset>,
}

impl CoreBuildManifest {
    pub fn new(plugin_id: impl Into<String>, artifact_kind: impl Into<String>) -> Result<Self> {
        let manifest = Self {
            plugin_id: plugin_id.into(),
            target: env::var("TARGET").context("TARGET is required in build.rs")?,
            artifact_kind: artifact_kind.into(),
            native_library: None,
            resources: Vec::new(),
            licenses: Vec::new(),
        };
        manifest.validate_plugin_id()?;
        Ok(manifest)
    }

    fn validate_plugin_id(&self) -> Result<()> {
        if self.plugin_id.is_empty()
            || self.plugin_id.len() > 128
            || !self
                .plugin_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            bail!("invalid plugin_id");
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_plugin_id()?;
        match self.artifact_kind.as_str() {
            "rust-cdylib" if self.native_library.is_none() => {}
            "native-shared"
                if self
                    .native_library
                    .as_ref()
                    .is_some_and(|p| p.is_absolute() && p.is_file()) => {}
            "native-static"
                if self.native_library.as_ref().is_some_and(|path| {
                    path.is_absolute()
                        && path.is_file()
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "a" || extension == "lib")
                }) => {}
            _ => bail!("artifact_kind and native_library disagree"),
        }
        for (assets, prefix) in [
            (&self.resources, "Resources/"),
            (&self.licenses, "licenses/"),
        ] {
            for asset in assets {
                if !asset.source.is_absolute() || !asset.source.exists() {
                    bail!("missing absolute build input: {}", asset.source.display());
                }
                if !(asset.dest.starts_with(prefix)
                    || (asset.dest == prefix.trim_end_matches('/') && asset.source.is_dir()))
                    || asset.dest.contains('\\')
                    || asset.dest.contains(':')
                    || asset
                        .dest
                        .split('/')
                        .any(|s| s.is_empty() || s == "." || s == "..")
                {
                    bail!("unsafe build asset destination: {}", asset.dest);
                }
            }
        }
        Ok(())
    }
}

pub fn write_core_build_manifest(manifest: &CoreBuildManifest) -> Result<PathBuf> {
    let out = PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is required in build.rs")?);
    write_core_build_manifest_to(manifest, &out)
}

pub fn write_core_build_manifest_to(manifest: &CoreBuildManifest, out: &Path) -> Result<PathBuf> {
    manifest.validate()?;
    fs::create_dir_all(out)?;
    let output_root = dunce::canonicalize(out)?;
    for asset in manifest.resources.iter().chain(&manifest.licenses) {
        let input = dunce::canonicalize(&asset.source)?;
        if !input.starts_with(&output_root) {
            println!("cargo:rerun-if-changed={}", input.display());
        }
    }
    let path = out.join("core-build.json");
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    fs::write(&path, bytes)?;
    Ok(path)
}

/// Redistributables and notices read from the producer's Cargo OUT_DIR.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct NativeInputs {
    pub runtime: Vec<PathBuf>,
    pub licenses: Vec<BuildAsset>,
    pub package_files: Vec<BuildAsset>,
    pub ndk_openmp: bool,
}

impl NativeInputs {
    pub const FILE: &str = "native-inputs.json";

    pub fn write(&self) -> Result<()> {
        fs::write(out_dir()?.join(Self::FILE), serde_json::to_vec(self)?)?;
        Ok(())
    }
}

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("ShinkiEMU workspace")
}

fn work_root() -> PathBuf {
    workspace().join(".work")
}

fn out_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is required")?))
}

fn pins() -> Result<inputs::Pins> {
    let path = workspace().join("build/dependencies.toml");
    println!("cargo:rerun-if-changed={}", path.display());
    Ok(toml::from_str(&fs::read_to_string(&path)?)?)
}

fn watch_env(key: &str) -> Option<std::ffi::OsString> {
    println!("cargo:rerun-if-env-changed={key}");
    env::var_os(key)
}

fn lock_file(path: &Path) -> Result<fs::File> {
    let file = fs::File::options().read(true).write(true).create(true).truncate(false).open(path)
        .with_context(|| format!("opening lock {}", path.display()))?;
    file.lock().with_context(|| format!("locking {}", path.display()))?;
    Ok(file)
}

fn capture(command: &mut Command) -> Result<String> {
    let output = command.output().with_context(|| format!("starting {command:?}"))?;
    if !output.status.success() {
        bail!("{command:?} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut hash = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex::encode(hash.finalize()))
}

pub fn android_ndk() -> Result<PathBuf> {
    [
        "ANDROID_NDK_HOME",
        "ANDROID_NDK_ROOT",
        "CARGO_NDK_HOME",
    ]
    .into_iter()
    .find_map(watch_env)
    .map(PathBuf::from)
    .or_else(|| {
        let sysroot = PathBuf::from(watch_env("CARGO_NDK_SYSROOT_PATH")?);
        Some(sysroot.ancestors().nth(5)?.to_owned())
    })
    .context("select an Android NDK with cargo ndk or ANDROID_NDK_HOME")
}

/// Restrict this build-script process's pkg-config probes to the installation.
pub fn configure_pkg_config(target: &str, paths: &[PathBuf], program: &Path) -> Result<()> {
    let target_key = target.replace('-', "_");
    let paths = env::join_paths(paths)?;
    // Build scripts call this before starting external probes and own the
    // process; this does not modify Cargo's or another crate's environment.
    unsafe {
        env::set_var(format!("PKG_CONFIG_{target}"), program);
        env::set_var(format!("PKG_CONFIG_PATH_{target}"), "");
        env::set_var(format!("PKG_CONFIG_LIBDIR_{target}"), paths);
        env::set_var("PKG_CONFIG_ALLOW_CROSS", "1");
        for name in ["PKG_CONFIG_SYSROOT_DIR", "PKG_CONFIG_SYSTEM_LIBRARY_PATH"] {
            for key in [name.to_owned(), format!("{name}_{target}"), format!("{name}_{target_key}"), format!("TARGET_{name}")] {
                env::remove_var(key);
            }
        }
    }
    Ok(())
}

fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("starting {}", command.get_program().to_string_lossy()))?;
    if !status.success() {
        bail!(
            "{} exited with {status}",
            command.get_program().to_string_lossy()
        );
    }
    Ok(())
}

/// Keep native build/download caches and temporary files in an owned work tree.
fn configure_native_environment(work_root: &Path, command: &mut Command) -> Result<()> {
    let absolute = std::path::absolute(work_root)?;
    let work_root = dunce::simplified(&absolute);
    let binary_cache = work_root.join("vcpkg-binary-cache");
    let downloads = work_root.join("vcpkg-downloads");
    for directory in [&binary_cache, &downloads] {
        for ancestor in directory.ancestors() {
            match fs::symlink_metadata(ancestor) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        bail!("linked native cache path: {}", ancestor.display());
                    }
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        if metadata.file_attributes() & 0x400 != 0 {
                            bail!("reparse native cache path: {}", ancestor.display());
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        fs::create_dir_all(directory)?;
    }
    command
        .env("VCPKG_DEFAULT_BINARY_CACHE", binary_cache)
        .env("VCPKG_DOWNLOADS", downloads)
        .env("VCPKG_MAX_CONCURRENCY", "4");
    Ok(())
}

/// Merge the native build's archives and publish its prepared static link closure to Cargo.
pub fn link_apple_static_core(
    core: &str,
    native: &Path,
    installations: &[PathBuf],
) -> Result<PathBuf> {
    fn archives(
        directory: &Path,
        staged: &Path,
        result: &mut std::collections::BTreeSet<PathBuf>,
    ) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if entry.path() != staged {
                    archives(&entry.path(), staged, result)?;
                }
            } else if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "a")
            {
                result.insert(fs::canonicalize(entry.path())?);
            }
        }
        Ok(())
    }
    let target = env::var("TARGET")?;
    let architecture = match target.split('-').next() {
        Some("aarch64") => "arm64",
        Some("x86_64") => "x86_64",
        _ => bail!("unsupported Apple static target {target}"),
    };
    if !target.contains("apple") {
        bail!("Apple static linking requires an Apple target");
    }
    let mut inputs = std::collections::BTreeSet::new();
    archives(native, &native.join("install"), &mut inputs)?;
    if inputs.is_empty() {
        bail!("native core {core} produced no static archives");
    }
    let name = format!("shinki_{}_closure", core.replace(['.', '-'], "_"));
    let output = native
        .parent()
        .context("native output parent")?
        .join(format!("lib{name}.a"));
    run(Command::new("xcrun")
        .args(["libtool", "-static", "-arch_only", architecture, "-o"])
        .arg(&output)
        .args(inputs))?;
    println!(
        "cargo:rustc-link-search=native={}",
        output.parent().unwrap().display()
    );
    println!("cargo:rustc-link-lib=static={name}");
    link_static_installations(installations)?;
    println!("cargo:rustc-link-lib=c++");
    println!("cargo:rustc-link-lib=framework=Foundation");
    Ok(output)
}

/// Link the installed native archives and their target pkg-config closure on Android.
pub fn link_android_static_core(native: &Path, installations: &[PathBuf]) -> Result<()> {
    if !env::var("TARGET")?.ends_with("-linux-android") {
        bail!("Android static linking requires an Android target");
    }
    link_static_installations(&[native.join("install")])?;
    link_static_installations(installations)?;
    println!("cargo:rustc-link-lib=c++_shared");
    for library in [
        "android", "log", "dl", "m", "EGL", "GLESv3", "OpenSLES", "mediandk",
    ] {
        println!("cargo:rustc-link-lib={library}");
    }
    Ok(())
}

fn link_static_installations(installations: &[PathBuf]) -> Result<()> {
    let target = env::var("TARGET")?;
    let dirs = installations.iter().flat_map(|installation| [
        installation.join("lib/pkgconfig"), installation.join("share/pkgconfig"),
    ]).filter(|path| path.is_dir()).collect::<Vec<_>>();
    if !dirs.is_empty() {
        let installed = installations.iter()
            .filter_map(|prefix| prefix.parent())
            .find(|root| root.file_name().is_some_and(|name| name == "installed"));
        let program = vcpkg::pkg_config_program(&target, installed)?;
        configure_pkg_config(&target, &dirs, &program)?;
    }
    for installation in installations {
        let libraries = installation.join("lib");
        println!("cargo:rustc-link-search=native={}", libraries.display());
        let mut entries = fs::read_dir(&libraries)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "a") {
                let stem = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .context("archive name")?;
                let library = stem.strip_prefix("lib").context("native archive prefix")?;
                println!("cargo:rustc-link-lib=static={library}");
            }
        }
        for directory in [
            installation.join("lib/pkgconfig"),
            installation.join("share/pkgconfig"),
        ] {
            if !directory.is_dir() {
                continue;
            }
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.extension().is_some_and(|extension| extension == "pc") {
                    pkg_config::Config::new().statik(true).probe(
                        path.file_stem()
                            .and_then(|value| value.to_str())
                            .context("pkg-config name")?,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Configure an OUT_DIR-owned native tree, then build the requested real targets.
/// Cross builds must supply an explicit CMake toolchain; no host/target guessing.
pub fn configure_cmake(
    source: &Path,
    definitions: &[(String, String)],
    targets: &[&str],
) -> Result<PathBuf> {
    let host = env::var("HOST").context("HOST is required")?;
    let target = env::var("TARGET").context("TARGET is required")?;
    let source =
        dunce::canonicalize(source).context("canonicalizing native CMake source directory")?;
    let output = PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is required")?);
    let out = dunce::simplified(&output).join("native");
    let profile = if env::var("PROFILE")? == "release" {
        "Release"
    } else {
        "Debug"
    };
    for name in [
        "CMAKE",
        "CMAKE_GENERATOR",
        "CMAKE_TOOLCHAIN_FILE",
        "CC",
        "CXX",
        "PATH",
        "LIB",
        "INCLUDE",
        "VCToolsInstallDir",
        "HOST",
        "TARGET",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed={}", source.display());
    if targets.is_empty() {
        bail!("CMake requires a real native target");
    }
    if host != target
        && env::var_os("CMAKE_TOOLCHAIN_FILE").is_none()
        && !definitions.iter().any(|(k, _)| k == "CMAKE_TOOLCHAIN_FILE")
    {
        bail!("cross CMake build {host} -> {target} requires CMAKE_TOOLCHAIN_FILE");
    }
    let cmake = env::var_os("CMAKE").unwrap_or_else(|| "cmake".into());
    let work_root = work_root();
    let mut configure = Command::new(&cmake);
    configure_native_environment(&work_root, &mut configure)?;
    configure.env("VCPKG_DISABLE_METRICS", "1");
    configure
        .arg("-S")
        .arg(source)
        .arg("-B")
        .arg(&out)
        .arg(format!("-DCMAKE_BUILD_TYPE={profile}"))
        .arg(format!(
            "-DCMAKE_INSTALL_PREFIX={}",
            out.join("install").display()
        ))
        .arg("-DFETCHCONTENT_FULLY_DISCONNECTED=ON")
        .arg("-DFETCHCONTENT_UPDATES_DISCONNECTED=ON")
        .arg("-DVCPKG_MANIFEST_INSTALL=OFF");
    let definition = |name: &str| definitions.iter().find_map(|(key, value)| (key == name).then_some(value));
    if target != host
        && let (Some(installed), Some(triplet)) = (
            definition("VCPKG_INSTALLED_DIR"),
            definition("VCPKG_TARGET_TRIPLET"),
        )
    {
        let prefix = Path::new(installed).join(triplet);
        let program = vcpkg::pkg_config_program(&target, Some(Path::new(installed)))?;
        configure
            .arg(format!("-DPKG_CONFIG_EXECUTABLE={}", program.display()))
            .env("PKG_CONFIG_PATH", "")
            .env("PKG_CONFIG_LIBDIR", env::join_paths([prefix.join("lib/pkgconfig"), prefix.join("share/pkgconfig")])?)
            .env_remove("PKG_CONFIG_SYSROOT_DIR");
    }
    if let Some(generator) = env::var_os("CMAKE_GENERATOR") {
        configure.arg("-G").arg(generator);
    } else if target.ends_with("-linux-android") {
        configure.args(["-G", "Ninja"]);
    } else if cfg!(windows)
        && target.ends_with("windows-msvc")
        && env::var_os("VCToolsInstallDir").is_some()
        && env::var_os("LIB").is_some()
        && env::var_os("INCLUDE").is_some()
        && let Some(ninja) = env::var_os("PATH").and_then(|paths| {
            env::split_paths(&paths)
                .map(|directory| directory.join("ninja.exe"))
                .find(|path| path.is_file())
        })
    {
        configure
            .args(["-G", "Ninja"])
            .arg(format!("-DCMAKE_MAKE_PROGRAM={}", ninja.display()));
    }
    if !definitions.iter().any(|(key, _)| key == "CMAKE_TOOLCHAIN_FILE")
        && let Some(toolchain) = env::var_os("CMAKE_TOOLCHAIN_FILE")
    {
        configure.arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={}",
            dunce::simplified(Path::new(&toolchain)).display()
        ));
    }
    for (key, value) in definitions {
        configure.arg(format!(
            "-D{key}={}",
            dunce::simplified(Path::new(value)).display()
        ));
    }
    run(&mut configure)?;
    let mut build = Command::new(cmake);
    configure_native_environment(&work_root, &mut build)?;
    build.env("VCPKG_DISABLE_METRICS", "1");
    build
        .arg("--build")
        .arg(&out)
        .arg("--config")
        .arg(profile)
        .arg("--target")
        .args(targets);
    if let Some(jobs) = env::var_os("NUM_JOBS") {
        build.arg("--parallel").arg(jobs);
    }
    run(&mut build)?;
    Ok(out)
}

/// Generate an MSVC import library from the official runtime DLL's exports.
pub fn generate_import_library(dll: &Path, out_dir: &Path, target: &str) -> Result<PathBuf> {
    if !target.ends_with("windows-msvc") {
        bail!("MSVC import libraries require a windows-msvc target");
    }
    let bytes = fs::read(dll).with_context(|| format!("reading {}", dll.display()))?;
    let file = object::File::parse(bytes.as_slice())?;
    let machine = match (target.split('-').next(), file.architecture()) {
        (Some("x86_64"), object::Architecture::X86_64) => "X64",
        (Some("aarch64"), object::Architecture::Aarch64) => "ARM64",
        (Some("i686"), object::Architecture::I386) => "X86",
        _ => bail!("DLL architecture does not match {target}"),
    };
    if file.format() != object::BinaryFormat::Pe {
        bail!("import source must be a PE DLL");
    }
    let name = dll
        .file_name()
        .and_then(|v| v.to_str())
        .context("DLL has no UTF-8 filename")?;
    if !name
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        bail!("unsafe DLL filename");
    }
    let mut exports = Vec::new();
    for export in file.exports()? {
        if let object::read::NameOrOrdinal::Name(name) = export?.name() {
            exports.push(std::str::from_utf8(name)?.to_owned());
        }
    }
    exports.sort();
    exports.dedup();
    if exports.is_empty() {
        bail!("DLL has no named exports");
    }
    if exports.iter().any(|s| {
        s.is_empty()
            || s.bytes()
                .any(|c| c.is_ascii_whitespace() || b"\";=".contains(&c))
    }) {
        bail!("unsafe DLL export name");
    }
    fs::create_dir_all(out_dir)?;
    let stem = dll
        .file_stem()
        .and_then(|name| name.to_str())
        .context("DLL has no UTF-8 stem")?;
    let definition = out_dir.join(format!("{stem}.def"));
    let library = out_dir.join(format!("{stem}.lib"));
    fs::write(
        &definition,
        format!("LIBRARY {name}\nEXPORTS\n{}\n", exports.join("\n")),
    )?;
    let tool = cc::windows_registry::find_tool(target, "lib.exe")
        .context("MSVC lib.exe not found; install Visual C++ build tools for the target")?;
    run(tool
        .to_command()
        .arg("/NOLOGO")
        .arg(format!("/MACHINE:{machine}"))
        .arg(format!("/DEF:{}", definition.display()))
        .arg(format!("/OUT:{}", library.display())))?;
    if !library.is_file() {
        bail!("lib.exe did not produce {}", library.display());
    }
    println!("cargo:rerun-if-changed={}", dll.display());
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_assets_do_not_publish_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("license");
        fs::write(&source, b"license").unwrap();
        let mut manifest = CoreBuildManifest {
            plugin_id: "test".into(),
            target: "x86_64-pc-windows-msvc".into(),
            artifact_kind: "rust-cdylib".into(),
            native_library: None,
            resources: vec![],
            licenses: vec![BuildAsset {
                source,
                dest: "licenses/../escape".into(),
            }],
        };
        assert!(write_core_build_manifest_to(&manifest, &temp.path().join("out")).is_err());
        assert!(!temp.path().join("out/core-build.json").exists());
        manifest.licenses[0].dest = "licenses/test/LICENSE".into();
        let path = write_core_build_manifest_to(&manifest, &temp.path().join("out")).unwrap();
        let decoded: CoreBuildManifest = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        decoded.validate().unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn dotted_dll_import_library_links_and_executes_a_real_consumer() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("dependency.rs");
        fs::write(&source, "#![no_std]\n#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! {loop{}}\n#[unsafe(no_mangle)] pub extern \"C\" fn dependency_answer()->u32 {42}\n").unwrap();
        let dll = temp.path().join("libpython3.12.dll");
        let status = Command::new("rustc")
            .arg(&source)
            .arg("--edition=2024")
            .args([
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
                "-C",
                "link-arg=/NOENTRY",
                "-o",
            ])
            .arg(&dll)
            .status()
            .unwrap();
        assert!(status.success());
        let host = Command::new("rustc").arg("-vV").output().unwrap();
        let version = String::from_utf8(host.stdout).unwrap();
        let target = version
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .unwrap();
        let imports = temp.path().join("imports");
        generate_import_library(&dll, &imports, target).unwrap();
        let consumer = temp.path().join("consumer.rs");
        fs::write(&consumer, "#[link(name=\"libpython3.12\")] unsafe extern \"C\" {fn dependency_answer()->u32;}\nfn main(){if unsafe {dependency_answer()} != 42 {std::process::exit(2);}}\n").unwrap();
        let executable = temp.path().join("consumer.exe");
        let status = Command::new("rustc")
            .arg(&consumer)
            .arg("--edition=2024")
            .arg(format!("-Lnative={}", imports.display()))
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(Command::new(&executable).status().unwrap().success());
    }
}

#[cfg(test)]
mod cargo_regressions {
    use super::*;

    #[test]
    fn unchanged_generated_assets_leave_the_real_cargo_library_fresh() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("Cargo.toml"), format!(
            "[package]\nname=\"generated-asset-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\nbuild=\"build.rs\"\n[lib]\ncrate-type=[\"cdylib\"]\n[workspace]\n[build-dependencies]\nshinki-emu-build={{path={:?}}}\n",
            env!("CARGO_MANIFEST_DIR")
        )).unwrap();
        fs::write(
            temp.path().join("LICENSE"),
            "Public synthetic fixture: CC0-1.0\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("src/lib.rs"),
            "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
        )
        .unwrap();
        fs::write(temp.path().join("build.rs"), r##"
use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let generated = out.join("generated.rs");
    std::fs::write(&generated, "#[unsafe(no_mangle)] pub extern \"C\" fn generated_answer()->u32 {42}\n").unwrap();
    let mut manifest = CoreBuildManifest::new("fixture", "rust-cdylib").unwrap();
    manifest.resources.push(BuildAsset {source: generated, dest: "Resources/generated.rs".into()});
    manifest.licenses.push(BuildAsset {
        source: std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("LICENSE"),
        dest: "licenses/fixture/LICENSE".into(),
    });
    write_core_build_manifest(&manifest).unwrap();
}
"##).unwrap();
        let cargo = |args: &[&str]| {
            let output = Command::new(env!("CARGO"))
                .current_dir(temp.path())
                .args(args)
                .env("CARGO_TARGET_DIR", temp.path().join("target"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        cargo(&["generate-lockfile", "--offline"]);
        let first = cargo(&["build", "--locked", "--offline", "--message-format=json"]);
        let second = cargo(&["build", "--locked", "--offline", "--message-format=json"]);
        let library = |bytes: &[u8]| {
            String::from_utf8_lossy(bytes)
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .find(|message| {
                    message["reason"] == "compiler-artifact"
                        && message["target"]["name"] == "generated_asset_fixture"
                })
                .expect("Cargo must report the actual generated library")
        };
        assert_eq!(library(&first)["fresh"], false);
        assert_eq!(library(&second)["fresh"], true);
    }
}
