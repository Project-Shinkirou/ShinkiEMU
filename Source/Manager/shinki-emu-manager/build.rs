use std::{env, process::Command};

use sha2::{Digest, Sha256};

#[path = "../../../build/adv32_candidate.rs"]
mod adv32_candidate;
#[path = "../../../build/core_adapters.rs"]
mod core_adapters;

fn write_runtime_icon() {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(
        include_bytes!("../../../Assets/Branding/shinkiemu-icon.svg"),
        &usvg::Options::default(),
    )
    .expect("SHINKI_EMU_MANAGER_ICON_INVALID");
    let mut pixmap =
        tiny_skia::Pixmap::new(64, 64).expect("SHINKI_EMU_MANAGER_ICON_DIMENSIONS_INVALID");
    pixmap.fill(tiny_skia::Color::from_rgba8(18, 50, 110, 255));
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(64.0 / tree.size().width(), 64.0 / tree.size().height()),
        &mut pixmap.as_mut(),
    );
    let output = env::var_os("OUT_DIR").expect("SHINKI_EMU_MANAGER_OUT_DIR_MISSING");
    std::fs::write(
        std::path::PathBuf::from(output).join("window-icon.rgba"),
        pixmap.data(),
    )
    .expect("SHINKI_EMU_MANAGER_WINDOW_ICON_WRITE_FAILED");
}

#[cfg(windows)]
fn embed_windows_icon() {
    use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
    use resvg::{tiny_skia, usvg};
    use std::{fs, io::Cursor};

    let tree = usvg::Tree::from_data(
        include_bytes!("../../../Assets/Branding/shinkiemu-icon.svg"),
        &usvg::Options::default(),
    )
    .expect("SHINKI_EMU_MANAGER_ICON_INVALID");
    let output = env::var_os("OUT_DIR").expect("SHINKI_EMU_MANAGER_OUT_DIR_MISSING");
    let mut icons = IconDir::new(ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let mut pixmap =
            tiny_skia::Pixmap::new(size, size).expect("SHINKI_EMU_MANAGER_ICON_DIMENSIONS_INVALID");
        pixmap.fill(tiny_skia::Color::from_rgba8(18, 50, 110, 255));
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(
                size as f32 / tree.size().width(),
                size as f32 / tree.size().height(),
            ),
            &mut pixmap.as_mut(),
        );
        let image = IconImage::read_png(Cursor::new(
            pixmap
                .encode_png()
                .expect("SHINKI_EMU_MANAGER_ICON_ENCODING_FAILED"),
        ))
        .expect("SHINKI_EMU_MANAGER_ICON_PNG_INVALID");
        icons
            .add_entry(IconDirEntry::encode(&image).expect("SHINKI_EMU_MANAGER_ICON_ENTRY_FAILED"));
    }
    let icon_path = std::path::PathBuf::from(output).join("shinkiemu.ico");
    icons
        .write(fs::File::create(&icon_path).expect("SHINKI_EMU_MANAGER_ICON_CREATE_FAILED"))
        .expect("SHINKI_EMU_MANAGER_ICON_WRITE_FAILED");
    winresource::WindowsResource::new()
        .set_icon(icon_path.to_str().expect("icon path"))
        .compile()
        .expect("SHINKI_EMU_MANAGER_ICON_RESOURCE_FAILED");
}

fn main() {
    println!("cargo:rerun-if-env-changed=SHINKI_EMU_PRODUCT_VERSION");
    println!("cargo:rerun-if-env-changed=SHINKI_EMU_PRODUCT_SOURCE_COMMIT");
    let explicit_version = env::var_os("SHINKI_EMU_PRODUCT_VERSION").is_some();
    let version = match env::var("SHINKI_EMU_PRODUCT_VERSION") {
        Ok(version) => version,
        Err(env::VarError::NotPresent) => env::var("CARGO_PKG_VERSION").expect("package version"),
        Err(error) => panic!("SHINKI_EMU_PRODUCT_VERSION: {error}"),
    };
    semver::Version::parse(&version).expect("SHINKI_EMU_PRODUCT_VERSION must be SemVer");
    match env::var("SHINKI_EMU_PRODUCT_SOURCE_COMMIT") {
        Ok(commit) => {
            assert!(
                explicit_version,
                "source identity requires an explicit product version"
            );
            assert!(
                commit.len() == 40
                    && commit
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
                "SHINKI_EMU_PRODUCT_SOURCE_COMMIT must be a full lowercase Git SHA"
            );
            println!("cargo:rustc-env=SHINKI_EMU_COMPILED_SOURCE_COMMIT={commit}");
        }
        Err(env::VarError::NotPresent) => {
            println!("cargo:rustc-env=SHINKI_EMU_COMPILED_SOURCE_COMMIT=")
        }
        Err(error) => panic!("SHINKI_EMU_PRODUCT_SOURCE_COMMIT: {error}"),
    }
    let target = env::var("TARGET").expect("build target");
    println!("cargo:rustc-env=SHINKI_EMU_BUILD_TARGET={target}");
    println!("cargo:rustc-env=SHINKI_EMU_PRODUCT_VERSION={version}");
    write_embedded_core_identities();
    println!("cargo:rerun-if-changed=../../../Assets/Branding/shinkiemu-icon.svg");
    write_runtime_icon();
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        shinki_emu_build::inputs::dxc(std::path::Path::new(
            "../../../Assets/Effects/dxc.lock.json",
        ))
        .expect("prepare pinned DirectX shader compiler runtime");
    }
    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon();
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        link_android_compiler_runtime();
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos") {
        let target = shinki_emu_build::ohos::Target::from_rust_target(&target)
            .expect("unsupported OHOS Manager target");
        println!("cargo:rustc-env=SHINKI_EMU_OHOS_ABI={}", target.abi());
        build_ohos_glue();
    }
    for asset in [
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

fn link_android_compiler_runtime() {
    // Native embedded cores use compiler-rt helpers (including libffi's
    // instruction-cache flush). Rust's compiler_builtins is not the NDK's
    // compiler-rt archive, and allowing unresolved cdylib symbols postpones
    // this link failure until System.loadLibrary on the device.
    let compiler = cc::Build::new().get_compiler();
    let output = compiler
        .to_command()
        .arg("--print-libgcc-file-name")
        .output()
        .expect("Android compiler runtime lookup");
    assert!(
        output.status.success(),
        "Android compiler runtime lookup failed"
    );
    let archive = String::from_utf8(output.stdout).expect("Android compiler runtime path");
    let archive = archive.trim();
    assert!(
        std::path::Path::new(archive).is_file(),
        "Android compiler runtime archive missing: {archive}"
    );
    println!("cargo:rustc-link-arg-cdylib={archive}");
    println!("cargo:rustc-link-arg-cdylib=-Wl,--no-undefined");
}

fn write_embedded_core_identities() {
    let root =
        std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
            .join("../../..");
    let inventory = root.join("build/cores.toml");
    let lock = root.join("Cargo.lock");
    for path in [
        &inventory,
        &lock,
        &root.join("build/core_adapters.rs"),
        &root.join("build/adv32_candidate.rs"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let inventory: toml::Value =
        toml::from_str(&std::fs::read_to_string(inventory).expect("core inventory"))
            .expect("valid core inventory");
    let lock: toml::Value =
        toml::from_str(&std::fs::read_to_string(lock).expect("workspace lockfile"))
            .expect("valid workspace lockfile");
    let target = env::var("TARGET").expect("build target");
    let mut identities = serde_json::Map::new();
    let os = env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    let ios_frameworks = os == "ios";
    let mut frameworks = Vec::new();
    let mut registry = String::from("&[\n");
    let ohos_bundled = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos")
        && env::var_os("CARGO_FEATURE_OHOS_BUNDLED_MUSICA").is_some();
    let optional_cmvs = !matches!(os.as_str(), "android" | "ios")
        && env::var_os("CARGO_FEATURE_STATIC_CMVS").is_some();
    let optional_adv32 = env::var_os("CARGO_FEATURE_STATIC_ADV32").is_some();
    for core in inventory["cores"].as_array().expect("cores") {
        let id = core["id"].as_str().expect("core id");
        let embedded = core
            .get("static_manager_targets")
            .and_then(toml::Value::as_array)
            .is_some_and(|targets| targets.iter().any(|value| value.as_str() == Some(&target)));
        let framework = ios_frameworks
            && core.get("ios_framework").and_then(toml::Value::as_bool) == Some(true);
        if framework {
            frameworks.push(id);
        }
        let linked = (embedded
            || (optional_cmvs && id == "shinki.emu.cmvs")
            || adv32_candidate::linked(core, &target, optional_adv32))
            && !(ohos_bundled && id == "shinki.emu.musica");
        if linked {
            use std::fmt::Write;
            let adapter = core_adapters::adapter_id(id).replace('-', "_");
            let query = match id {
                "shinki.emu.adv32" => "adv32_query",
                "shinki.emu.cmvs" => "cmvs_query",
                "shinki.emu.musica" => "musica_query",
                "krkr2" => "query",
                _ => "shinki_emu_query",
            };
            writeln!(registry, "    ({id:?}, shinki_emu_{adapter}::{query}),")
                .expect("write query registry");
        }
        if (embedded || framework)
            && let Some(attribution) = core.get("attribution")
            && let Some(adapter) = core_adapters::locked_adapter(&lock, id)
        {
            identities.insert(id.to_owned(), serde_json::json!({
                "upstream": attribution["upstream_identity"],
                "adapter": {"repo": adapter.repo, "branch": attribution["adapter_branch"], "commit": adapter.commit},
                "release_sequence": attribution["release_sequence"],
            }));
        }
    }
    let output = std::path::PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join("embedded-core-identities.json");
    std::fs::write(
        &output,
        serde_json::to_vec(&identities).expect("identity JSON"),
    )
    .expect("embedded identity output");
    registry.push_str("]\n");
    std::fs::write(output.with_file_name("static-core-queries.rs"), registry)
        .expect("static query registry");
    if ios_frameworks {
        std::fs::write(
            output.with_file_name("ios-framework-core-ids.rs"),
            format!("&{frameworks:?}\n"),
        )
        .expect("iOS Framework core IDs");
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// NAPI/XComponent glue from `Platforms/OHOS/native` and the
/// system libraries it uses. xtask supplies the clang target/sysroot.
fn build_ohos_glue() {
    let bridge = "../../../Platforms/OHOS/native/bridge.c";
    println!("cargo:rerun-if-changed={bridge}");
    cc::Build::new()
        .file(bridge)
        .flag("-Wall")
        .flag("-Werror")
        .compile("shinki_ohos_glue");
    for library in [
        "ace_napi.z",
        "ace_ndk.z",
        "native_window",
        "native_buffer",
        "native_vsync",
        "hilog_ndk.z",
    ] {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
}
