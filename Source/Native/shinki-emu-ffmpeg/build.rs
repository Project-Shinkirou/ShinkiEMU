use shinki_emu_build::vcpkg;

fn main() -> anyhow::Result<()> {
    let installation = vcpkg::install_shared("ffmpeg")?;
    let inputs = installation.report("ffmpeg", &std::env::var("TARGET")?)?;
    installation.publish(&inputs)
}
