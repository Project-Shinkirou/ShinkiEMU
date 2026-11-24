#[path = "build_support/catalog.rs"]
mod catalog;

use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("../../..");
    let cores = root.join("build/cores.toml");
    let targets = root.join("build/targets.toml");
    for input in [&cores, &targets] {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    println!("cargo:rerun-if-changed=build_support/catalog.rs");
    println!("cargo:rerun-if-changed=../../../build/android_dynamic.rs");
    println!("cargo:rerun-if-changed=../../../build/adv32_candidate.rs");
    let generated = catalog::generate(
        &fs::read_to_string(cores).expect("core inventory"),
        &fs::read_to_string(targets).expect("target inventory"),
        &env::var("TARGET").expect("build target"),
        env::var_os("CARGO_FEATURE_STATIC_ADV32").is_some(),
    ).expect("valid static core catalog");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("static-core-catalog.rs"), generated).expect("static catalog output");
}
