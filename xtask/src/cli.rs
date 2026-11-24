use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cargo xtask",
    about = "ShinkiEMU build, packaging, release and metadata tools",
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

#[derive(Args)]
pub struct Build {
    #[arg(long)]
    pub target: String,
    #[arg(long, conflicts_with_all=["manager", "all_cores"])]
    pub core: Option<String>,
    #[arg(long, conflicts_with = "all_cores")]
    pub manager: bool,
    #[arg(long)]
    pub all_cores: bool,
    #[arg(long)]
    pub release: bool,
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
    Build(Build),
    Check {
        #[arg(long)]
        target: String,
        #[command(flatten)]
        selection: Selection,
    },
    Test {
        #[arg(long, conflicts_with = "manager")]
        core: Option<String>,
        #[arg(long)]
        manager: bool,
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
        /// Build and package one Android Manager ABI with the local debug key.
        /// Requires a new output directory and no release signing configuration.
        #[arg(long, requires = "manager", conflicts_with = "core")]
        qa_single_abi: bool,
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
    Ci {
        #[arg(long)]
        target: String,
        #[arg(long)]
        component: String,
        #[arg(long)]
        report: PathBuf,
    },
}
