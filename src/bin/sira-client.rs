use anyhow::{anyhow, bail};
use shlex::Shlex;
use sira::core::Action;
use std::env;
use std::process::Command;

fn main() -> anyhow::Result<()> {
    let yaml = env::args().nth(1).ok_or(anyhow!(
        "sira-client requires one argument, an action in YAML format, but none was provided"
    ))?;
    let action: Action = serde_yaml::from_str(&yaml)?;

    use Action::*;
    match action {
        Shell(commands) => {
            for command_string in commands {
                let mut words = Shlex::new(&command_string);
                let command = words
                    .next()
                    .ok_or(anyhow!("sira-client received a blank shell command"))?;
                let args: Vec<_> = words.collect();
                let child_exit_status = Command::new(command).args(&args).spawn()?.wait()?;
                if !child_exit_status.success() {
                    let exit_code_message = match child_exit_status.code() {
                        Some(i) => format!("exit code {i}"),
                        None => "error".to_string(),
                    };
                    bail!("command exited with {exit_code_message}: {command_string}");
                }
            }
        }
        LineInFile { .. } => bail!("not yet implemented"),
        Upload { .. } | Download { .. } => {
            bail!("this action is implemented on sira, not sira-client");
        }
    }
    Ok(())
}
