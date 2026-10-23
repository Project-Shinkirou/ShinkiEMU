use crate::{build, cores::Inventory, dependencies, package};
use anyhow::Result;
use serde::Serialize;
use std::{fs, io::Write, path::Path};
#[derive(Serialize)]
struct Report<'a> {
    schema: &'static str,
    component: &'a str,
    target: &'a str,
    status: &'static str,
    reason: Option<String>,
    artifact: Option<std::path::PathBuf>,
}
pub fn run(inv: &Inventory, target: &str, component: &str, path: &Path) -> Result<()> {
    let mut blocked = inv.blocked(component, target)?.map(str::to_owned);
    if target.contains("ohos") && std::env::var_os("OHOS_SDK_NATIVE").is_none() {
        if let Some(reason) = blocked.as_mut() {
            reason.push_str(" Missing prerequisite: OHOS_SDK_NATIVE.");
        }
    }
    let mut report = Report {
        schema: "shinki.emu.ci.v1",
        component,
        target,
        status: "blocked",
        reason: blocked.clone(),
        artifact: None,
    };
    let result = if blocked.is_some() {
        Ok(())
    } else {
        (|| -> Result<()> {
            dependencies::doctor(inv, target, Some(component).filter(|s| *s != "manager"))?;
            let built = build::execute(inv, target, component, "build", true)?
                .expect("build returns artifact");
            report.status = "built";
            report.artifact = Some(built.library);
            let version =
                std::env::var("SHINKI_EMU_CI_VERSION").unwrap_or_else(|_| "0.1.0-beta.1".into());
            // Restricted binaries are not uploaded, even as Actions artifacts.
            if component != "manager" && !inv.core(component)?.publish_blocked.is_empty() {
                report.status = "publish_blocked";
                report.reason = Some(inv.core(component)?.publish_blocked.clone());
                report.artifact = None;
                return Ok(());
            }
            let out = inv.root.join(".work/ci").join(target).join(component);
            let staged = package::package(inv, target, component, &version, &out)?;
            let archive = out.with_extension("zip");
            package::zip_entries(&package::tree(&staged)?, &archive)?;
            report.artifact = Some(archive);
            Ok(())
        })()
    };
    if let Err(e) = &result {
        report.status = "failed";
        report.reason = Some(format!("{e:#}"));
        report.artifact = None;
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(summary)?;
        writeln!(
            f,
            "### {component} / {target}\n\n**{}**\n\n{}\n\nArtifact: {}\n",
            report.status,
            report.reason.as_deref().unwrap_or(""),
            report
                .artifact
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "null".into())
        )?;
    }
    println!("{}", serde_json::to_string(&report)?);
    result
}
