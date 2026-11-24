use crate::cores::Inventory;
use anyhow::{Context, Result, bail};
use shinki_emu_build::ohos::Target as OhosTarget;
use std::{env, fs, path::{Path, PathBuf}, process::Command};

pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("starting {}", cmd.get_program().to_string_lossy()))?;
    if !status.success() {
        bail!(
            "{} failed with {status}",
            cmd.get_program().to_string_lossy()
        );
    }
    Ok(())
}
pub fn capture(cmd: &mut Command) -> Result<Vec<u8>> {
    let output = cmd
        .output()
        .with_context(|| format!("starting {}", cmd.get_program().to_string_lossy()))?;
    if !output.status.success() {
        bail!(
            "{} failed: {}",
            cmd.get_program().to_string_lossy(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}
pub fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("Project-Shinkirou-xtask")
        .timeout(std::time::Duration::from_secs(600))
        .build()?)
}
pub fn ndk_root(inv: &Inventory) -> Result<PathBuf> {
    #[derive(serde::Deserialize)]
    struct AndroidPin { android_ndk_version: String }
    let pin: AndroidPin = toml::from_str(&fs::read_to_string(inv.root.join("build/dependencies.toml"))?)?;
    let root = env::var_os("ANDROID_NDK_HOME")
        .or_else(|| env::var_os("ANDROID_NDK_ROOT"))
        .or_else(|| env::var_os("CARGO_NDK_HOME"))
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ANDROID_HOME")
                .or_else(|| env::var_os("ANDROID_SDK_ROOT"))
                .map(|p| PathBuf::from(p).join("ndk").join(&pin.android_ndk_version))
        })
        .context("set ANDROID_NDK_HOME or ANDROID_HOME")?;
    validate_ndk_revision(&root, &pin.android_ndk_version)?;
    Ok(root)
}

fn validate_ndk_revision(root: &Path, required: &str) -> Result<()> {
    let props = fs::read_to_string(root.join("source.properties"))
        .context("NDK source.properties missing")?;
    if !props.lines().any(|s| {
        s.split_once('=')
            .is_some_and(|(k, v)| k.trim() == "Pkg.Revision" && v.trim() == required)
    }) {
        bail!("NDK {required} required at {}", root.display());
    }
    Ok(())
}

pub(crate) fn ohos_sdk_native() -> Result<PathBuf> {
    let sdk = env::var_os("OHOS_SDK_NATIVE")
        .map(PathBuf::from)
        .or_else(|| {
            cfg!(windows)
                .then(|| PathBuf::from("D:/Huawei/DevEco Studio/sdk/default/openharmony/native"))
        })
        .context("OHOS_SDK_NATIVE must point to the DevEco native SDK")?;
    for required in [
        "build/cmake/ohos.toolchain.cmake",
        "sysroot/usr/include",
        "llvm/bin",
    ] {
        if !sdk.join(required).exists() {
            bail!("OHOS native SDK missing {required}: {}", sdk.display());
        }
    }
    Ok(sdk)
}
pub(crate) fn apply_ohos_environment(inv: &Inventory, target: OhosTarget, cmd: &mut Command) -> Result<()> {
    configure_ohos_command(&inv.root, &ohos_sdk_native()?, target, cmd)
}

fn configure_ohos_command(root: &Path, sdk: &Path, target: OhosTarget, cmd: &mut Command) -> Result<()> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let tools = sdk.join("llvm/bin");
    let target_key = target.rust_target().replace('-', "_");
    let sysroot = sdk.join("sysroot");
    let llvm_target = target.llvm_target();
    let sysroot_libs = sysroot.join("usr/lib").join(llvm_target);
    if !sysroot_libs.is_dir() {
        bail!("OHOS native SDK missing target sysroot libraries: {}", sysroot_libs.display());
    }
    let clang_args = format!(
        "--target={llvm_target} --sysroot=\"{}\"",
        sysroot.display()
    );
    // The cc crate splits *FLAGS on whitespace without honoring quotes, and
    // the DevEco path contains a space: pass the flags as a response file.
    let response = root.join(".work/ohos").join(target.rust_target()).join("clang-target.rsp");
    fs::create_dir_all(response.parent().context("response file parent")?)?;
    fs::write(
        &response,
        format!(
            "--target={llvm_target}\n\"--sysroot={}\"\n",
            sysroot.to_string_lossy().replace('\\', "/")
        ),
    )?;
    let cc_flags = format!("@{}", response.to_string_lossy().replace('\\', "/"));
    if cc_flags.contains(char::is_whitespace) {
        bail!("the repository path must not contain whitespace for OHOS builds");
    }
    // Rust's default linker name is a shell wrapper in the SDK. Use the clang
    // driver with target and sysroot for every final link.
    // With --target, Cargo applies these flags to target crates only.
    let existing = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_else(|_| {
        env::var("RUSTFLAGS")
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("\x1f")
    });
    let link = format!("-C\x1flink-arg={cc_flags}");
    cmd.env(
        "CARGO_ENCODED_RUSTFLAGS",
        if existing.is_empty() {
            link
        } else {
            format!("{existing}\x1f{link}")
        },
    );
    cmd.env("OHOS_SDK_NATIVE", sdk)
        .env(
            format!("CARGO_TARGET_{}_LINKER", target_key.to_ascii_uppercase()),
            tools.join(format!("clang{suffix}")),
        )
        .env(
            format!("CC_{target_key}"),
            tools.join(format!("clang{suffix}")),
        )
        .env(
            format!("CXX_{target_key}"),
            tools.join(format!("clang++{suffix}")),
        )
        .env(
            format!("AR_{target_key}"),
            tools.join(format!("llvm-ar{suffix}")),
        )
        .env(format!("CFLAGS_{target_key}"), &cc_flags)
        .env(format!("CXXFLAGS_{target_key}"), &cc_flags)
        // OHOS ships libc++ (no libstdc++); apps bundle libc++_shared.so.
        // Read by cc and luau0-src, which otherwise default to stdc++.
        .env(format!("CXXSTDLIB_{target_key}"), "c++_shared")
        .env(
            format!("BINDGEN_EXTRA_CLANG_ARGS_{target_key}"),
            &clang_args,
        )
        // The cmake crate drives the SDK's ohos.toolchain.cmake; use the
        // SDK's CMake and Ninja, not the host default generator (NMake on
        // Windows).
        .env(format!("CMAKE_GENERATOR_{target_key}"), "Ninja")
        .env(
            "PATH",
            env::join_paths(
                std::iter::once(sdk.join("build-tools/cmake/bin"))
                    .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
            )?,
        );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ohos_compiler_routes_keep_response_files_isolated() -> Result<()> {
        let output = crate::temporary::directory()?;
        let sdk = output.path().join("DevEco SDK");
        let mut responses = Vec::new();
        for (target, llvm) in [
            (OhosTarget::Arm64, "aarch64-linux-ohos"),
            (OhosTarget::X64, "x86_64-linux-ohos"),
        ] {
            fs::create_dir_all(sdk.join("sysroot/usr/lib").join(llvm))?;
            let mut command = Command::new("cargo");
            configure_ohos_command(output.path(), &sdk, target, &mut command)?;
            let vars: std::collections::BTreeMap<_, _> = command.get_envs().collect();
            let key = target.rust_target().replace('-', "_");
            let get = |name: &str| vars[std::ffi::OsStr::new(name)].unwrap().to_str().unwrap();
            let flags = get(&format!("CFLAGS_{key}"));
            assert_eq!(flags, get(&format!("CXXFLAGS_{key}")));
            assert!(get("CARGO_ENCODED_RUSTFLAGS").ends_with(&format!("link-arg={flags}")));
            let bindgen = get(&format!("BINDGEN_EXTRA_CLANG_ARGS_{key}"));
            assert!(bindgen.starts_with(&format!("--target={llvm} ")));
            assert!(bindgen.ends_with(&format!("--sysroot=\"{}\"", sdk.join("sysroot").display())));
            let rsp = PathBuf::from(flags.strip_prefix('@').unwrap());
            assert!(fs::read_to_string(&rsp)?.starts_with(&format!("--target={llvm}\n")));
            responses.push((rsp, llvm));
        }
        assert_ne!(responses[0].0, responses[1].0);
        for (rsp, llvm) in responses {
            assert!(fs::read_to_string(rsp)?.starts_with(&format!("--target={llvm}\n")));
        }
        Ok(())
    }

    #[test]
    fn ndk_requires_the_exact_stable_revision() {
        let temp = crate::temporary::directory().unwrap();
        let properties = temp.path().join("source.properties");
        for revision in ["30.0.16248370-beta2", "30.0.15729638", "29.0.14206865"] {
            fs::write(&properties, format!("Pkg.Revision = {revision}\n")).unwrap();
            assert!(validate_ndk_revision(temp.path(), "30.0.16248370").is_err());
        }
        fs::write(&properties, "Pkg.Desc = Android NDK\nPkg.Revision = 30.0.16248370\n").unwrap();
        validate_ndk_revision(temp.path(), "30.0.16248370").unwrap();
    }
}
