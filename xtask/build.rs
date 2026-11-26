use std::{env, error::Error, fmt::Write as _, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=Cargo.toml");
    let manifest: toml::Table = toml::from_str(&fs::read_to_string("Cargo.toml")?)?;
    let dependencies = manifest["dependencies"]
        .as_table()
        .expect("Cargo dependencies table");
    let mut hosts: Vec<_> = dependencies
        .keys()
        .filter(|name| name.ends_with("-android-host"))
        .collect();
    hosts.sort_unstable();

    let mut source = String::from("&[\n");
    for host in hosts {
        let name = host.replace('-', "_");
        writeln!(
            source,
            "    (::{name}::PLUGIN_ID, ::{name}::stage, ::{name}::ACTIVITY_ADAPTER_CLASS),"
        )?;
    }
    source.push_str("]\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    fs::write(out.join("android_hosts.rs"), source)?;
    Ok(())
}
