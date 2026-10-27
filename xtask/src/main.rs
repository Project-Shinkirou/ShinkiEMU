mod apple;
mod binary;
mod build;
mod ci;
mod cli;
mod cores;
mod dependencies;
mod metadata;
mod package;
mod release;
mod rust_licenses;
mod sources;
#[cfg(test)]
mod tests;
use anyhow::{Result, bail};
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
        cli::Command::AppleImage { target } => apple::image(root, &target)?,
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
            if args.all_cores {
                dependencies::prepare_all(&inv, &args.target, args.allow_dirty_sources)?;
            } else {
                dependencies::prepare(
                    &inv,
                    &args.target,
                    args.core.as_deref(),
                    args.allow_dirty_sources,
                )?;
            }
        }
        cli::Command::Build(args) => {
            inv.target(&args.target)?;
            if args.all_cores {
                let mut built = false;
                for core in &inv.cores {
                    if let Some(reason) = inv.blocked(&core.id, &args.target)? {
                        eprintln!("{}: blocked: {reason}", core.id);
                        continue;
                    }
                    build::execute(&inv, &args.target, &core.id, "build", args.release)?;
                    built = true;
                }
                if !built {
                    bail!(
                        "no standalone core supports {}; select --manager for static integrations",
                        args.target
                    );
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
        } => match target {
            Some(target) if !publish => release::prepare(&inv, &target, &component, &version)?,
            _ => release::publish(&inv, &component, &version)?,
        },
        cli::Command::Metadata { cmd } => metadata::run(cmd)?,
        cli::Command::Ci {
            target,
            component,
            report,
        } => ci::run(&inv, &target, &component, &report)?,
    }
    Ok(())
}
