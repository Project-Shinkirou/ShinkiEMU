use std::{env, process::Command};

use sha2::{Digest, Sha256};

fn main() {
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos") {
        build_ohos_glue();
    }
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

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// NAPI/XComponent and OHAudio glue from `Platforms/OHOS/native`, plus the
/// link arguments the DevEco clang driver needs for the Manager cdylib.
fn build_ohos_glue() {
    let sdk = std::path::PathBuf::from(
        env::var_os("OHOS_SDK_NATIVE").expect("SHINKI_EMU_MANAGER_BUILD_OHOS_SDK_NATIVE_MISSING"),
    );
    let sysroot = sdk.join("sysroot");
    let sources = ["../../../Platforms/OHOS/native/bridge.c", "../../../Platforms/OHOS/native/audio.c"];
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-env-changed=OHOS_SDK_NATIVE");
    cc::Build::new()
        .files(sources)
        .flag("--target=aarch64-linux-ohos")
        .flag(format!("--sysroot={}", sysroot.display()))
        .flag("-Wall")
        .flag("-Werror")
        .compile("shinki_ohos_glue");
    for library in ["ace_napi.z", "ace_ndk.z", "native_window", "native_buffer", "native_vsync", "ohaudio", "hilog_ndk.z"] {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
    println!("cargo:rustc-link-arg-cdylib=--target=aarch64-linux-ohos");
    println!("cargo:rustc-link-arg-cdylib=--sysroot={}", sysroot.display());
}
