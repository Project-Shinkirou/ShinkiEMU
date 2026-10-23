//! Host-side build support. This crate never invokes Cargo or downloads tools.
use anyhow::{bail, Context, Result};
use object::Object;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const SCHEMA: &str = "shinki.emu.core_build.v1";
pub const ABI: &str = "shinki.emu.native.c.v3";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildAsset {
    pub source: PathBuf,
    pub dest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreBuildManifest {
    pub schema: String,
    pub plugin_id: String,
    pub target: String,
    pub profile: String,
    pub abi: String,
    pub artifact_kind: String,
    pub native_library: Option<PathBuf>,
    pub resources: Vec<BuildAsset>,
    pub licenses: Vec<BuildAsset>,
}

impl CoreBuildManifest {
    pub fn new(plugin_id: impl Into<String>, artifact_kind: impl Into<String>) -> Result<Self> {
        Ok(Self {
            schema: SCHEMA.into(),
            plugin_id: plugin_id.into(),
            target: env::var("TARGET").context("TARGET is required in build.rs")?,
            profile: env::var("PROFILE").context("PROFILE is required in build.rs")?,
            abi: ABI.into(),
            artifact_kind: artifact_kind.into(),
            native_library: None,
            resources: Vec::new(),
            licenses: Vec::new(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA || self.abi != ABI {
            bail!("invalid core build protocol");
        }
        if self.plugin_id.is_empty()
            || self.plugin_id.len() > 128
            || !self
                .plugin_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            bail!("invalid plugin_id");
        }
        if self.target.is_empty() || !matches!(self.profile.as_str(), "debug" | "release") {
            bail!("invalid target/profile");
        }
        match self.artifact_kind.as_str() {
            "rust-cdylib" if self.native_library.is_none() => {}
            "native-shared"
                if self
                    .native_library
                    .as_ref()
                    .is_some_and(|p| p.is_absolute() && p.is_file()) => {}
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
    for asset in manifest.resources.iter().chain(&manifest.licenses) {
        println!("cargo:rerun-if-changed={}", asset.source.display());
    }
    let path = out.join("core-build.json");
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    fs::write(&path, bytes)?;
    Ok(path)
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

/// Configure an OUT_DIR-owned native tree, then build the requested real targets.
/// Cross builds must supply an explicit CMake toolchain; no host/target guessing.
pub fn configure_cmake(
    source: &Path,
    definitions: &[(String, String)],
    targets: &[&str],
) -> Result<PathBuf> {
    let host = env::var("HOST").context("HOST is required")?;
    let target = env::var("TARGET").context("TARGET is required")?;
    let out = PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is required")?).join("native");
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
        "VCPKG_ROOT",
        "BISON_EXECUTABLE",
        "SHINKI_EMU_VCPKG_INSTALLED",
        "SHINKI_EMU_VCPKG_TRIPLET",
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
    let mut configure = Command::new(&cmake);
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
    if let Some(generator) = env::var_os("CMAKE_GENERATOR") {
        configure.arg("-G").arg(generator);
    }
    if let Some(toolchain) = env::var_os("CMAKE_TOOLCHAIN_FILE") {
        configure.arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={}",
            toolchain.to_string_lossy()
        ));
    }
    if let Some(installed) = env::var_os("SHINKI_EMU_VCPKG_INSTALLED") {
        let installed = PathBuf::from(installed);
        if !installed.is_dir() {
            bail!(
                "native vcpkg installation missing: {}; run cargo xtask deps",
                installed.display()
            );
        }
        configure.arg(format!("-DVCPKG_INSTALLED_DIR={}", installed.display()));
    }
    if let Ok(triplet) = env::var("SHINKI_EMU_VCPKG_TRIPLET") {
        configure.arg(format!("-DVCPKG_TARGET_TRIPLET={triplet}"));
    }
    for (key, value) in definitions {
        configure.arg(format!("-D{key}={value}"));
    }
    run(&mut configure)?;
    let mut build = Command::new(cmake);
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
    let mut exports: Vec<String> = file
        .exports()?
        .iter()
        .map(|e| std::str::from_utf8(e.name()).map(str::to_owned))
        .collect::<std::result::Result<_, _>>()?;
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
    let stem = dll.file_stem().context("DLL has no stem")?;
    let definition = out_dir.join(stem).with_extension("def");
    let library = out_dir.join(stem).with_extension("lib");
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
            schema: SCHEMA.into(),
            abi: ABI.into(),
            plugin_id: "test".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
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
}
