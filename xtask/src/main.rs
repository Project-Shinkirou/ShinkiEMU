mod binary;
mod build;
mod ci;
mod cli;
mod cores;
mod dependencies;
mod metadata;
mod package;
mod release;
#[cfg(test)]
mod tests;
use anyhow::{bail, Result};
use clap::Parser;
fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = cli::Cli::parse();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask repository root");
    let inv = cores::Inventory::load(root)?;
    match cli.command {
        cli::Command::Matrix { json } => {
            let matrix = inv.matrix()?;
            if json {
                println!("{}", serde_json::to_string(&matrix)?);
            } else {
                for r in matrix.include {
                    println!(
                        "{}\t{}\t{}\t{}",
                        r.component,
                        r.target,
                        r.status,
                        r.reason.unwrap_or_default()
                    );
                }
            }
        }
        cli::Command::Doctor(args) => {
            dependencies::doctor(&inv, &args.target, args.core.as_deref())?
        }
        cli::Command::Deps(args) => {
            dependencies::prepare(&inv, &args.target, args.core.as_deref())?
        }
        cli::Command::Build(args) => {
            inv.target(&args.target)?;
            if args.all_cores {
                for core in &inv.cores {
                    if let Some(reason) = inv.blocked(&core.id, &args.target)? {
                        eprintln!("{}: blocked: {reason}", core.id);
                        continue;
                    }
                    build::execute(&inv, &args.target, &core.id, "build", args.release)?;
                }
            } else {
                let component = if args.manager {
                    "manager"
                } else {
                    args.core
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("select --core, --all-cores or --manager"))?
                };
                build::execute(&inv, &args.target, component, "build", args.release)?;
            }
        }
        cli::Command::Check { target, selection } => {
            build::execute(&inv, &target, selection.component(), "check", false)?;
        }
        cli::Command::Test { core, manager } => {
            if manager && core.is_some() {
                bail!("select one component");
            }
            build::tests(&inv, core.as_deref(), manager)?;
        }
        cli::Command::Package {
            target,
            selection,
            version,
            out,
        } => {
            println!(
                "{}",
                package::package(&inv, &target, selection.component(), &version, &out)?.display()
            );
        }
        cli::Command::Release {
            component,
            target,
            version,
            publish,
        } => release::run(&inv, &target, &component, &version, publish)?,
        cli::Command::Metadata { cmd } => metadata::run(cmd)?,
        cli::Command::Ci {
            target,
            component,
            report,
        } => ci::run(&inv, &target, &component, &report)?,
    }
    Ok(())
}
