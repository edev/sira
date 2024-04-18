//! Client-side logic for [Action::Script].

use crate::client;
use crate::core::Action;
use anyhow::{bail, Context};
use std::fs;
use std::io::Write;
use std::process::Command;

/// Implements client-side logic for [Action::Script].
///
/// Writes the script to a temporary file, runs it as the specified user, and then deletes it.
pub fn script(action: &Action) -> anyhow::Result<()> {
    let (user, contents) = match action {
        Action::Script {
            name: _,
            user,
            contents,
        } => (user, contents),
        _ => panic!("called script with an Action tht was not a Script: {action:?}"),
    };

    // We need a temporary file that the target user can access, so we can't put it in the Sira
    // user's SSH starting directory (e.g. their home directory). Bonus: this path should be
    // absolute, so we won't have issues looking it up in PATH later like we would with a file in
    // the current directory.
    let (mut script_file, script_path) = client::mktemp()?;

    script_file
        .write_all(contents.as_bytes())
        .context("failed to write script to temporary file")?;
    drop(script_file);

    // std::fs can chmod but not chown. We'll use our own, nicer interface for both.
    run("chmod", ["500", &script_path])?;
    run("chown", [user, &script_path])?;

    let result = run("sudo", ["-u", user, &script_path]);

    let _ = fs::remove_file(&script_path);
    result
}

fn run<const N: usize>(cmd: &str, args: [&str; N]) -> anyhow::Result<()> {
    // Descriptive form of command used for error output.
    let command = || format!("{cmd} {}", args.join(" "));

    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("failed to start command: {}", command()))?;

    if !status.success() {
        let error = match status.code() {
            Some(i) => format!("exit code {i}"),
            None => "error".to_string(),
        };
        bail!("command exited with {error}: {}", command());
    }
    Ok(())
}
