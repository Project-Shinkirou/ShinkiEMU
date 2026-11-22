use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Windowless ShinkiEMU C ABI and Manager UI runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(clap::Subcommand)]
enum Command {
    Run(shinki_emu_headless::Arguments),
    Compare(shinki_emu_headless::capture::CompareArguments),
    #[cfg(feature = "manager-ui")]
    Ui(shinki_emu_headless::ui::Arguments),
}
fn main() -> std::process::ExitCode {
    match Cli::parse().command {
        Command::Compare(arguments) => shinki_emu_headless::capture::compare_cli(arguments),
        Command::Run(arguments) => shinki_emu_headless::run_cli(arguments, &[]),
        #[cfg(feature = "manager-ui")]
        Command::Ui(arguments) => shinki_emu_headless::ui::run_cli(arguments),
    }
}
