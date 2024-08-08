//! Provides a [tokio]-based [Plan] runner that runs on each host in parallel.

use crate::core::plan::HostPlanIntoIter;
use crate::core::Action;
use crate::core::Plan;
use crate::crypto::{self, SigningOutcome};
use anyhow::bail;
use std::panic;
use tokio::task::JoinSet;

pub mod client;
use client::*;

pub mod report;
use report::*;

/// The name of the key used for signing actions before they're sent from `sira` to `sira-client`.
pub const ACTION_SIGNING_KEY: &str = "action";

/// Runs a [Plan] on each of the [Plan]'s hosts in parallel.
///
/// If a host is unreachable, it will simply be skipped; the [Plan] will still run to completion on
/// every available host.
///
/// Similarly, if a host encounters an error, either due to a connection issue or an [Action] that
/// fails (e.g. an [Action::Command] that returns a non-zero exit code), that host will execute no
/// further [Action]s, but other hosts will run to completion.
///
/// # Returns
///
/// If all hosts run all of their [Action]s successfully, returns `Ok(())`. Otherwise, returns a
/// list of failure tuples of the form `(host, error)`. Hosts that do not appear in this error
/// return have successfully run their portions of the [Plan].
///
/// [Action]: crate::core::Action
/// [Action::Command]: crate::core::Action::Command
pub async fn run_plan(plan: Plan) -> Result<(), Vec<(String, anyhow::Error)>> {
    _run_plan(plan, ConnectionManager, Reporter::new()).await
}

/// Provides dependency injection for unit-testing [run_plan] without SSH, stdout, or stderr.
async fn _run_plan<
    C: ClientInterface + Send,
    CM: ManageClient<C> + Clone + Send + 'static,
    R: Report + Clone + Send + 'static,
>(
    plan: Plan,
    connection_manager: CM,
    reporter: R,
) -> Result<(), Vec<(String, anyhow::Error)>> {
    let mut host_plans = JoinSet::new();

    for host in plan.hosts() {
        let host_plan = plan.plan_for(&host).unwrap().into_iter();
        let cm = connection_manager.clone();
        let rep = reporter.clone();
        let _ = host_plans.spawn(async move {
            let status = run_host_plan(host.clone(), host_plan, cm, rep).await;
            (host, status)
        });
    }

    let mut errors = Vec::new();
    while let Some(join_result) = host_plans.join_next().await {
        if let Err(err) = join_result {
            if err.is_panic() {
                panic::resume_unwind(err.into_panic());
            } else {
                panic!("Tokio task failed to execute to completion; this should be impossible");
            }
        }

        let (host, status) = join_result.unwrap();
        if let Err(err) = status {
            errors.push((host, err));
        }
    }

    match errors.len() {
        0 => Ok(()),
        _ => Err(errors),
    }
}

/// Runs a [Plan] on a single host via [HostPlanIntoIter].
async fn run_host_plan<C: ClientInterface, CM: ManageClient<C>, R: Report + Clone>(
    host: String,
    plan: HostPlanIntoIter,
    mut connection_manager: CM,
    mut reporter: R,
) -> anyhow::Result<()> {
    let mut client = connection_manager.connect(&host).await?;

    for action in plan {
        let action = action.compile();
        let yaml = serde_yaml::to_string(&action).unwrap();

        // Lazily sign yaml only if needed.
        fn sign(yaml: &str) -> anyhow::Result<Option<Vec<u8>>> {
            match crypto::sign(yaml.as_bytes(), ACTION_SIGNING_KEY)? {
                SigningOutcome::Signed(sig) => Ok(Some(sig)),
                SigningOutcome::KeyNotFound => Ok(None),
            }
        }

        reporter.starting(&host, &action).await?;

        use Action::*;
        let output = match &action {
            Command(_) => client.command(&yaml, sign(&yaml)?).await?,
            LineInFile { .. } => client.line_in_file(&yaml, sign(&yaml)?).await?,
            Script { .. } => client.script(&yaml, sign(&yaml)?).await?,
            Upload { from, .. } => client.upload(from, &yaml, sign(&yaml)?).await?,
        };

        reporter.report(&host, &action, &output).await?;

        if !output.status.success() {
            // Adapted from crate::run_plan::report::_report()
            let exit_code_message = match output.status.code() {
                Some(i) => format!("exit code {i}"),
                None => "error".to_string(),
            };
            let action = title(&action);
            bail!("Action exited with {exit_code_message}: {action}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod test;
