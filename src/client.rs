//! Utilities for implementing logic on a managed node.
//!
//! This module is used writing client logic and has no interaction with SSH. The SSH connection to
//! the managed node runs through [mod@crate::run_plan].

use anyhow::{bail, Context};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::os::unix::ffi::OsStringExt;
use std::process::Command;

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

#[cfg(test)]
mod test;
