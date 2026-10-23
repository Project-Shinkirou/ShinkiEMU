use std::{env, fs, path::PathBuf, process::Command};

use sha2::{Digest, Sha256};

fn main() {
    for asset in [
        "../../../Assets/Effects/dxc.lock.json",
        "../../../Assets/Effects/Builtin/scale.hlsl",
        "../../../Assets/Effects/Builtin/sharpen.hlsl",
        "../../../Assets/Effects/Anime4K/restore_cnn_s.hlsl",
        "../../../Assets/Effects/Anime4K/upscale_cnn_x2_s.hlsl",
    ] {
        println!("cargo:rerun-if-changed={asset}");
    }
    let rustc = env::var_os("RUSTC").expect("SHINKI_EMU_MANAGER_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("SHINKI_EMU_MANAGER_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "SHINKI_EMU_MANAGER_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("SHINKI_EMU_MANAGER_BUILD_RUSTC_IDENTITY_NOT_UTF8")
        .lines()
        .filter(|line| {
            line.starts_with("release:")
                || line.starts_with("commit-hash:")
                || line.starts_with("host:")
                || line.starts_with("LLVM version:")
        })
        .collect::<Vec<_>>()
        .join(";");
    assert!(
        !identity.is_empty(),
        "SHINKI_EMU_MANAGER_BUILD_RUSTC_IDENTITY_EMPTY"
    );
    println!(
        "cargo:rustc-env=SHINKI_EMU_MANAGER_RUSTC_FINGERPRINT=sha256.{}",
        hex_sha256(identity.as_bytes())
    );
    println!(
        "cargo:rustc-env=SHINKI_EMU_TARGET={}",
        env::var("TARGET").expect("SHINKI_EMU_MANAGER_BUILD_TARGET_MISSING")
    );
    configure_static_ios_registration();
    for name in [
        "SHINKI_EMU_FAMILY_SIGNER_ID",
        "SHINKI_EMU_FAMILY_PUBLIC_KEY_HEX",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        println!(
            "cargo:rustc-env={name}={}",
            env::var(name).unwrap_or_default()
        );
    }
    println!("cargo:rerun-if-env-changed=RUSTC");
}

fn configure_static_ios_registration() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("SHINKI_EMU_OUT_DIR_MISSING"))
        .join("shinki-emu-rfvp-static-manifest.json");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        fs::write(output, b"{}").expect("SHINKI_EMU_STATIC_MANIFEST_STUB_WRITE_FAILED");
        return;
    }
    let manifest_path = env::var("SHINKI_EMU_RFVP_STATIC_MANIFEST")
        .expect("SHINKI_EMU_RFVP_STATIC_MANIFEST_MISSING");
    let archive_path =
        env::var("SHINKI_EMU_RFVP_STATIC_ARCHIVE").expect("SHINKI_EMU_RFVP_STATIC_ARCHIVE_MISSING");
    let manifest = fs::read(&manifest_path).expect("SHINKI_EMU_RFVP_STATIC_MANIFEST_READ_FAILED");
    let archive = fs::read(&archive_path).expect("SHINKI_EMU_RFVP_STATIC_ARCHIVE_READ_FAILED");
    assert!(
        !manifest.is_empty() && manifest.len() <= 1024 * 1024,
        "SHINKI_EMU_RFVP_STATIC_MANIFEST_BOUNDS"
    );
    assert!(!archive.is_empty(), "SHINKI_EMU_RFVP_STATIC_ARCHIVE_EMPTY");
    fs::write(output, manifest).expect("SHINKI_EMU_RFVP_STATIC_MANIFEST_COPY_FAILED");
    println!(
        "cargo:rustc-env=SHINKI_EMU_RFVP_STATIC_BINARY_HASH=sha256:{}",
        hex_sha256(&archive)
    );
    println!("cargo:rerun-if-changed={manifest_path}");
    println!("cargo:rerun-if-changed={archive_path}");
    println!("cargo:rerun-if-env-changed=SHINKI_EMU_RFVP_STATIC_MANIFEST");
    println!("cargo:rerun-if-env-changed=SHINKI_EMU_RFVP_STATIC_ARCHIVE");
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
