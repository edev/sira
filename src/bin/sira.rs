use anyhow::anyhow;
use sira::core::Plan;
use sira::run_plan;
use std::env;

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
