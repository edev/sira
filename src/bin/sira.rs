use anyhow::anyhow;
use sira::core::Plan;
use sira::run_plan;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manifest_files = [Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/sample.manifest")];
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
