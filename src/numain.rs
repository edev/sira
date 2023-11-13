use crate::core::plan::HostPlanIntoIter;
use crate::core::Plan;
use anyhow::bail;
use openssh::KnownHosts;
use std::io::{self, Write};
use std::process::Output;
use std::sync::Arc;
use tokio::process::Command;
use tokio::task;

pub async fn run_plan(plan: Plan) -> Result<(), Vec<(String, anyhow::Error)>> {
    let mut run_futures = Vec::new();
    for host in plan.hosts() {
        let host_plan = plan.plan_for(&host).unwrap().into_iter();
        run_futures.push((host.clone(), run_host_plan(host, host_plan)));
    }

    let mut errors = Vec::new();
    for (host, future) in run_futures {
        if let Err(err) = future.await {
            errors.push((host, err));
        }
    }

    match errors.len() {
        0 => Ok(()),
        _ => Err(errors),
    }
}

async fn run_host_plan(host: String, plan: HostPlanIntoIter) -> anyhow::Result<()> {
    let session = openssh::Session::connect_mux(&host, KnownHosts::Add).await?;

    // Minor optimization: we need to pass host to report() as an owned value so it can be moved to
    // a blocking task. Use an Arc instead of cloning Strings.
    let host = Arc::new(host);

    for action in plan {
        use crate::core::Action::*;
        match action.compile() {
            action @ Shell(_) | action @ LineInFile { .. } => {
                let yaml = serde_yaml::to_string(&action).unwrap();
                let output = session.command("sira-client").arg(&yaml).output().await?;
                report(host.clone(), yaml, output).await?;
            }
            ref action @ Upload { ref from, ref to } => {
                let to = format!("{host}:{to}");
                let output = scp(from, &to).await?;
                let yaml = serde_yaml::to_string(&action).unwrap();
                report(host.clone(), yaml, output).await?;
            }
            ref action @ Download { ref from, ref to } => {
                let from = format!("{host}:{from}");
                let output = scp(&from, to).await?;
                let yaml = serde_yaml::to_string(&action).unwrap();
                report(host.clone(), yaml, output).await?;
            }
        }
    }
    Ok(())
}

async fn scp(from: &str, to: &str) -> io::Result<Output> {
    Command::new("scp").arg(from).arg(to).output().await
}

async fn report(host: Arc<String>, yaml: String, output: Output) -> anyhow::Result<()> {
    task::spawn_blocking(move || {
        // Lock stdout and stderr for sane output ordering. For this same reason, we do not use Tokio's
        // async IO, which provides no locking mechanisms.
        let mut stdout = io::stdout();
        let mut stderr = io::stderr();

        writeln!(stdout, "Ran action on {host}:\n{yaml}")?;

        if !output.stdout.is_empty() {
            writeln!(
                stdout,
                "Captured stdout:\n{}",
                String::from_utf8_lossy(&output.stdout),
            )?;
        }

        if !output.stderr.is_empty() {
            writeln!(
                stderr,
                "Captured stderr:\n{}",
                String::from_utf8_lossy(&output.stderr),
            )?;
        }

        if !output.status.success() {
            let exit_code_message = match output.status.code() {
                Some(i) => format!("exit code {i}"),
                None => "error".to_string(),
            };
            bail!("action exited with {exit_code_message}:\n{yaml}");
        }

        // Put a space before the next command's report, since this one succeeded.
        writeln!(stdout)?;
        Ok(())
    })
    .await?
}
