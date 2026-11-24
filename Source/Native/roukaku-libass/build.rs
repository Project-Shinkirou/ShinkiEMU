use shinki_emu_build::{configure_pkg_config, vcpkg::install_shared};
use std::env;

fn main() -> anyhow::Result<()> {
    let installation = install_shared("subtitle")?;
    let target = env::var("TARGET")?;
    let inputs = installation.report("subtitle", &target)?;
    if target == "x86_64-pc-windows-msvc" {
        anyhow::ensure!(
            !env::var("CARGO_CFG_TARGET_FEATURE")?.split(',').any(|feature| feature == "crt-static"),
            "libass requires the MD CRT"
        );
        vcpkg::Config::new()
            .vcpkg_root(installation.install_root.clone())
            .target_triplet(&installation.triplet)
            .copy_dlls(false)
            .find_package("libass")?;
        println!("cargo:rustc-link-lib=gdi32");
        println!("cargo:rustc-link-lib=user32");
    } else {
        let program = installation.pkg_config_program(&target)?;
        configure_pkg_config(&target, &installation.pkg_config_dirs(), &program)?;
        pkg_config::Config::new()
            .statik(target != "x86_64-unknown-linux-gnu")
            .exactly_version("0.17.5")
            .probe("libass")?;
    }
    println!("cargo::metadata=include_dirs={}", installation.prefix.join("include").display());
    println!("cargo::metadata=lib_dirs={}", installation.prefix.join("lib").display());
    installation.publish(&inputs)
}
