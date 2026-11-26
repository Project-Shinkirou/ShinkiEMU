use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cargo xtask",
    about = "ShinkiEMU packaging, release, metadata and CI tools",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct Selection {
    #[arg(long)]
    pub core: Option<String>,
    #[arg(long)]
    pub manager: bool,
}
impl Selection {
    pub fn component(&self) -> &str {
        self.core.as_deref().unwrap_or("manager")
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Build a private Apple cross image from SHINKI_APPLE_SDK_DIR.
    AppleImage {
        #[arg(long)]
        target: String,
    },
    /// List every combination; `--json` emits the job matrix of supported rows only.
    Matrix {
        #[arg(long)]
        json: bool,
        /// Drop rows whose CI build another row already performs (the universal Android Manager).
        #[arg(long, requires = "json")]
        ci: bool,
        /// Filter to components affected by changes since this Git ref.
        #[arg(long, requires = "json")]
        since: Option<String>,
        /// Release platform scope; omitted means all inventory platforms.
        #[arg(long, requires = "json", conflicts_with = "since")]
        scope: Option<crate::release::Scope>,
    },
    /// Build and stage a package. Install the cargo-about host tool first.
    Package {
        #[arg(long)]
        target: String,
        #[command(flatten)]
        selection: Selection,
        #[arg(long, default_value = "")]
        version: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Prepare one target's release files, or publish every prepared target.
    Release {
        #[arg(long)]
        component: String,
        #[arg(long, required_unless_present_any = ["publish", "dry_run"], conflicts_with_all = ["publish", "dry_run"])]
        target: Option<String>,
        #[arg(long, default_value = "")]
        version: String,
        /// Apply the same platform scope to preparation and publication.
        #[arg(long)]
        scope: Option<crate::release::Scope>,
        #[arg(long)]
        publish: bool,
        /// Print the release version and targets without building.
        #[arg(long, conflicts_with = "publish")]
        dry_run: bool,
    },
    Metadata {
        #[command(subcommand)]
        cmd: crate::metadata::MetadataCommand,
    },
    /// Build and package one CI matrix row.
    Ci {
        #[arg(long)]
        target: String,
        #[arg(long)]
        component: String,
        #[arg(long)]
        report: PathBuf,
    },
}
