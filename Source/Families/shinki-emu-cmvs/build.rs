use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
use std::{env, path::PathBuf};

fn acm_helper() {
    println!("cargo:rustc-check-cfg=cfg(cmvs_acm32_available)");
    println!("cargo:rerun-if-changed=native/acm32.c");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86") {
        cc::Build::new()
            .file("native/acm32.c")
            .define("CMVS_ACM_LIBRARY", None)
            .compile("cmvs_acm32");
        println!("cargo:rustc-link-lib=msacm32");
        println!("cargo:rustc-cfg=cmvs_acm32_available");
        return;
    }
    let Some(compiler) = cc::windows_registry::find_tool("i686-pc-windows-msvc", "cl.exe") else {
        println!(
            "cargo:warning=CMVS MP3 requires cmvs-acm32.exe; MSVC x86 tools are unavailable on this build host"
        );
        return;
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let executable = out.join("cmvs-acm32.exe");
    let object = out.join("cmvs-acm32.obj");
    let status = compiler
        .to_command()
        .arg("/nologo")
        .arg("/O2")
        .arg("/MT")
        .arg("/W4")
        .arg(format!("/Fo{}", object.display()))
        .arg(format!("/Fe{}", executable.display()))
        .arg("native/acm32.c")
        .arg("/link")
        .arg("/MACHINE:X86")
        .arg("msacm32.lib")
        .status()
        .expect("launching the CMVS x86 ACM helper compiler");
    assert!(status.success(), "building the CMVS x86 ACM helper failed");
    let profile = out
        .ancestors()
        .nth(3)
        .expect("Cargo profile output directory");
    std::fs::copy(&executable, profile.join("cmvs-acm32.exe"))
        .expect("staging the CMVS x86 ACM helper");
    println!("cargo:rustc-cfg=cmvs_acm32_available");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    acm_helper();
    for input in ["HOST", "TARGET", "PROFILE"] {
        println!("cargo:rerun-if-env-changed={input}");
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is missing")?)
            .join("../../..")
            .canonicalize()?;
    let mut manifest = CoreBuildManifest::new("shinki.emu.cmvs", "rust-cdylib")?;
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        manifest.licenses.push(BuildAsset {
            source: root.join(file),
            dest: format!("licenses/shinki.emu.cmvs/{file}"),
        });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
