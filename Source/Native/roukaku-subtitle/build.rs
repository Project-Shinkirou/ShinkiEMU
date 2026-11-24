use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/bridge.c");
    let target = env::var("TARGET").unwrap();
    let windows = env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let (includes, metadata) = if windows {
        assert_eq!(
            env::var("TARGET").unwrap(),
            "x86_64-pc-windows-msvc",
            "roukaku-subtitle requires the pinned x64 MSVC native installation on Windows"
        );
        assert!(
            !env::var("CARGO_CFG_TARGET_FEATURE")
                .unwrap_or_default()
                .split(',')
                .any(|feature| feature == "crt-static"),
            "roukaku-subtitle requires the MD CRT (no +crt-static)"
        );
        let root = PathBuf::from(
            env::var_os("DEP_SHINKI_LIBASS_VCPKG_ROOT").expect("libass build metadata"),
        );
        let lib = vcpkg::Config::new()
            .vcpkg_root(root)
            .target_triplet(&env::var("DEP_SHINKI_LIBASS_TRIPLET").unwrap())
            .cargo_metadata(false)
            .copy_dlls(false)
            .find_package("libass")
            .expect("install libass 0.17.5 in the isolated subtitle vcpkg root");
        assert!(
            lib.is_static,
            "libass must be static; only fribidi may be dynamic"
        );
        for required in ["libass", "freetype", "fribidi", "harfbuzz"] {
            assert!(
                lib.ports.iter().any(|port| port == required),
                "libass installation is missing a required transitive port"
            );
        }
        (lib.include_paths, lib.cargo_metadata)
    } else {
        // A real native installation is required on other platforms too. Never
        // provide a no-op renderer for a missing native dependency.
        let paths = env::split_paths(&env::var_os("DEP_SHINKI_LIBASS_PKG_CONFIG_DIRS").unwrap()).collect::<Vec<_>>();
        let program = PathBuf::from(env::var_os("DEP_SHINKI_LIBASS_PKG_CONFIG").unwrap());
        shinki_emu_build::configure_pkg_config(&target, &paths, &program).unwrap();
        let lib = pkg_config::Config::new()
            .statik(target != "x86_64-unknown-linux-gnu")
            .exactly_version("0.17.5")
            .probe("libass")
            .expect("roukaku-subtitle requires native libass exactly 0.17.5");
        // Let pkg-config emit its complete platform metadata, including
        // absolute library files, frameworks and private linker arguments.
        (lib.include_paths, Vec::new())
    };
    let mut bridge = cc::Build::new();
    bridge
        .file("src/bridge.c")
        .includes(&includes)
        .warnings(true);
    if windows {
        bridge.static_crt(false);
    }
    // bridge.c checks LIBASS_VERSION against 0x01705000 using the actual
    // installed header; it also uses the official track layout and va_list.
    bridge.compile("roukaku_subtitle_bridge");
    for line in metadata {
        println!("{line}");
    }
    if windows {
        // vcpkg-rs omits system libraries used by libass's desktop font
        // provider, even though provider lookup is disabled at runtime.
        println!("cargo:rustc-link-lib=gdi32");
        println!("cargo:rustc-link-lib=user32");
    }
}
