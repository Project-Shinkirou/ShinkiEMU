mod affected;
mod apple;
mod binary;
mod build;
mod ci;
mod cli;
mod cores;
mod dependencies;
mod icon;
mod metadata;
mod ohos;
mod package;
mod release;
mod rust_licenses;
mod temporary;
#[cfg(test)]
mod tests;
use anyhow::Result;
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
        cli::Command::Matrix {
            json,
            ci,
            since,
            scope,
        } => {
            if json {
                let affected = since
                    .map(|reference| affected::since(&inv, &reference))
                    .transpose()?;
                let mut matrix = inv.job_matrix(ci, affected.as_ref())?;
                matrix
                    .include
                    .retain(|row| scope.unwrap_or_default().includes(&row.platform));
                println!("{}", serde_json::to_string(&matrix)?);
            } else {
                for r in inv.rows()? {
                    println!(
                        "{}\t{}\t{}\t{}",
                        r.component,
                        r.target,
                        if r.blocked.is_some() {
                            "blocked"
                        } else {
                            "supported"
                        },
                        r.blocked.unwrap_or_default()
                    );
                }
            }
        }
        cli::Command::Package {
            target,
            selection,
            version,
            out,
            qa_single_abi,
            qa_cores,
            qa_prepare_cores,
            qa_from,
        } => {
            let path = if qa_prepare_cores {
                package::prepare_android_qa_cores(&inv, &target, &version, &out, &qa_cores)?
            } else if qa_single_abi {
                package::package_android_qa(&inv, &target, &version, &out, &qa_cores, &qa_from)?
            } else {
                package::package(&inv, &target, selection.component(), &version, &out)?
            };
            println!("{}", path.display());
        }
        cli::Command::Release {
            component,
            target,
            version,
            scope,
            publish: _,
            dry_run,
        } => {
            let scope = scope.unwrap_or_default();
            if dry_run {
                release::dry_run(&inv, &component, &version, scope)?;
            } else if let Some(target) = target {
                release::prepare(&inv, &target, &component, &version, scope)?;
            } else {
                release::publish(&inv, &component, &version, scope)?;
            }
        }
        cli::Command::Metadata { cmd } => metadata::run(cmd)?,
        cli::Command::Ci {
            target,
            component,
            report,
        } => ci::run(&inv, &target, &component, &report)?,
    }
    Ok(())
}
