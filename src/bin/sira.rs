use anyhow::anyhow;
use sira::core::Plan;
use sira::run_plan;
use std::env;

// TODO Write a full control node application instead of this stub.
// TODO Write a proper UI.
// TODO Add any options or modes that make sense, e.g. --validate.

// TODO Consider setting up an advanced testing system involving compiling VMs or containers.
//
// There is a lot that we can't test right now involving interactions between the sira and
// sira-client binaries.

// TODO Consider what integration or system tests make sense and add them.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manifest_files: Vec<String> = env::args().skip(1).collect();
    let plan = Plan::from_manifest_files(&manifest_files)?;
    match run_plan(plan).await {
        Err(errors) => {
            for (host, error) in errors {
                eprintln!("Error on host \"{host}\": {error}");
            }
            Err(anyhow!("one or more hosts encountered an error"))
        }
        Ok(()) => Ok(()),
    }
}
