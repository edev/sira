//! Client-side logic for [Action::Script].

use crate::core::Action;
use anyhow::{bail, Context};
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
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
    // user's SSH starting directory (e.g. their home directory). Bonus: this path is absolute, so
    // we won't have issues looking it up in PATH later like we would with a file in the current
    // directory.
    let script_path = {
        let output = Command::new("mktemp").output()?;
        if output.status.success() {
            // Trim any trailing white space, e.g. a trailing newline.
            let mut path = String::from_utf8(output.stdout)
                .expect("mktemp returned a path that was not UTF-8");
            path.truncate(path.trim_end().len());
            path
        } else {
            bail!(
                "mktemp exited with error:\n{:?}",
                OsString::from_vec(output.stderr),
            );
        }
    };

    fs::write(&script_path, contents.as_bytes())
        .context("failed to write script to temporary file")?;

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
