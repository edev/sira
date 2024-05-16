//! Utilities for implementing logic on a managed node.
//!
//! This module is used writing client logic and has no interaction with SSH. The SSH connection to
//! the managed node runs through [mod@crate::run_plan].

use anyhow::{bail, Context};
use shlex::Quoter;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::os::unix::ffi::OsStringExt;
use std::process::{Command, Output};
use std::sync::OnceLock;

/// Invokes the `mktemp` system utility.
///
/// `mktemp` might write a newline after the returned path, so this function trims trailing white
/// space and returns only the path itself. If parsing fails due to an unexpected output format
/// from `mktemp`, this function should fail to open the temporary file as a [File] and then return
/// an informative error. However, an implementation of `mktemp` that differs sufficiently from the
/// expected format might cause unexpected results.
///
/// For safety, this function parses the path as a UTF-8 [String] in order to clean it up rather
/// than implementing custom, non-UTF-8 parsing logic.
///
/// # Returns
///
/// Returns the temporary file as both a [File] opened for writing and the path to the file.
///
/// # Errors
///
/// Returns an error if `mktemp` cannot be run for any reason, exits with an error, or returns a
/// path that cannot be parsed as UTF-8.
pub fn mktemp() -> anyhow::Result<(File, String)> {
    let output = Command::new("mktemp").output()?;
    if !output.status.success() {
        bail!(
            "mktemp exited with error:\n{:?}",
            OsString::from_vec(output.stderr),
        );
    }

    let mut path = String::from_utf8(output.stdout.clone()).with_context(|| {
        format!(
            "mktemp returned a path that was not UTF-8: {}",
            String::from_utf8_lossy(&output.stdout),
        )
    })?;

    // Trim any trailing white space, e.g. a trailing newline.
    path.truncate(path.trim_end().len());
    let path = path;

    // Critical: if the file does not exist, then opening the file MUST fail. Otherwise, in the
    // event that the path mktemp actually created and the value of `path` differ (presumably due
    // to incorrect parsing in the code above), we will open a different file that might be under
    // an attacker's control.
    //
    // For extra safety, we explicitly set critical options rather than depending on the documented
    // defaults, just in case they eventually change.
    let mut file_options = OpenOptions::new();
    file_options.create(false);
    file_options.read(true);
    file_options.truncate(true);
    file_options.write(true);
    let file = file_options
        .open(&path)
        .with_context(|| format!("failed to open a file created by mktemp: {path}"))?;
    Ok((file, path))
}

/// Runs a command as a new process and waits for it to complete.
///
/// Standard input, output, and error are inherited from the parent process.
///
/// # Returns
///
/// Returns `Ok(())` if the command runs successfully (as indicated by exit status).
///
/// # Errors
///
/// Returns an error if the command cannot be run for any reason or exits with an error.
///
/// # Example
///
/// ```
/// use sira::client;
///
/// // Runs command: cargo doc --document-private-items
/// assert!(client::run("cargo", &["doc", "--document-private-items"]).is_ok());
/// ```
pub fn run<C: AsRef<OsStr>, A: AsRef<OsStr>>(cmd: C, args: &[A]) -> anyhow::Result<()> {
    // If needed for error output, join cmd and args with spaces to construct a user-friendly
    // representation of the command.
    //
    // There are at least three likely ways this information might be used:
    // 1. In separate fields as part of this Rust function and calling code
    // 2. In a YAML file
    // 3. In the user's shell
    //
    // These all present the information a bit differently, so there is no canonical representation
    // to apply. We just want to make a best effort to indicate to the user what went wrong.
    let command = || {
        // Build a Vec of the command and its arguments as Strings.
        let mut components = Vec::with_capacity(args.len() + 1);
        components.push(cmd.as_ref().to_string_lossy().to_string());
        components.extend(
            args.iter()
                .map(|a| a.as_ref().to_string_lossy().to_string()),
        );

        // Try to use shlex to properly quote the string. If that fails, naively join with spaces.
        match Quoter::new().join(components.iter().map(|s| &s[..])) {
            Ok(s) => s,
            Err(_) => components.join(" "),
        }
    };

    let status = Command::new(&cmd)
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

/// Invokes the `whoami` system utility.
///
/// Memorizes the identity on first run and returns the cached result on all subsequent calls.
///
/// # Panics
///
/// Panics if `whoami` cannot be called, `whoami` returns an error, or the returned user name
/// is not UTF-8.
pub fn whoami() -> &'static str {
    static COMPUTED: OnceLock<String> = OnceLock::new();

    COMPUTED
        .get_or_init(|| {
            match Command::new("whoami")
                .output()
                .expect("error calling `whoami`")
            {
                Output { status, stdout, .. } if status.success() => String::from_utf8(stdout)
                    .expect("user names should be UTF-8")
                    .trim()
                    .to_string(),
                Output { stderr, .. } => {
                    panic!("error calling `whoami`: {:?}", OsString::from_vec(stderr))
                }
            }
        })
        .as_ref()
}

#[cfg(test)]
mod test;
