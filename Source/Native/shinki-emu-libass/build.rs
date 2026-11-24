use shinki_emu_build::vcpkg;

fn main() -> anyhow::Result<()> {
    let installation = vcpkg::install_shared("subtitle")?;
    let inputs = installation.report("subtitle", &std::env::var("TARGET")?)?;
    installation.publish(&inputs)
}
