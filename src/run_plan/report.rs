//! Reports the outcome of each [Action] that runs on a client.
//!
//! The arrangement of this module is a bit unusual. Instead of presenting a generic method that
//! the user calls with either a real or a fake trait implementation, the user chooses either a
//! real or a fake trait implementation and calls that trait's method, [Report::report]. This trait
//! method calls a private method that provides all of the logic to report the outcome of an
//! [Action] to writers that can be either real or fake stdout and stderr writers. This odd
//! arrangement allows the real trait implementation to lock stdout and stderr just before
//! reporting and release the locks as soon as reporting is done. Locking this way allows most of
//! the program to write to stdout and stderr freely but prevents the output of multiple hosts
//! from getting mangled if they try to report at the same time.
//!
//! [Action]: crate::core::Action

use crate::core::Action;
use async_trait::async_trait;
use std::fmt::Display;
use std::io::{self, Write};
use std::ops::DerefMut;
use std::process::Output;
use tokio::task;

/// Prints feedback about each [Action] run on a client to stdout/stderr to keep the user informed.
///
/// [Action]: crate::core::Action
#[async_trait]
pub trait Report {
    /// Reports that an action is about to commence.
    async fn starting(&mut self, host: &str, action: &Action) -> io::Result<()>;

    /// Reports the outcome of an action.
    async fn report(&mut self, host: &str, action: &Action, output: &Output) -> io::Result<()>;
}

/// The real, production-ready [Report] implementation. Uses the real stdout/stderr.
#[derive(Clone, Debug)]
pub struct Reporter;

#[async_trait]
impl Report for Reporter {
    async fn starting(&mut self, host: &str, action: &Action) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        task::block_in_place(move || _starting(&mut stdout, host, action))
    }

    async fn report(&mut self, host: &str, action: &Action, output: &Output) -> io::Result<()> {
        // Lock stdout and stderr for sane output ordering. For this same reason, we do not use
        // Tokio's async IO, which provides no locking mechanisms.
        //
        // We need to release the locks as soon as we're done reporting rather than holding them
        // across invocations, so we construct them here instead of storing them in the struct.
        let mut stdout = io::stdout().lock();
        let mut stderr = io::stderr().lock();
        task::block_in_place(move || _report(&mut stdout, &mut stderr, host, action, output))
    }
}

/// Generates a one-line identifier for an [Action], suitable for use as its title in user output.
pub fn title(action: &Action) -> String {
    use Action::*;
    match action {
        Command(vec) => {
            // It's unlikely that vec has more than one element, but that's not our concern.
            format!("command: {}", vec.join("; "))
        }
        LineInFile { line, path, .. } => format!("line_in_file ({path}): {line}"),
        Script { name, user, .. } => format!("script ({user}): {name}"),
        Upload { from, to, .. } => format!("upload: {from} -> {to}"),
    }
}

/// A testable method containing the logic for reporting the outcome of an [Action].
pub fn _report<OT: Write, ET: Write, O: DerefMut<Target = OT>, E: DerefMut<Target = ET>>(
    mut stdout: O,
    mut stderr: E,
    host: &str,
    action: &Action,
    output: &Output,
) -> io::Result<()> {
    fn write_indented(
        mut writer: impl Write,
        header: impl Display,
        content: impl AsRef<str>,
    ) -> io::Result<()> {
        //                1234
        writeln!(writer, "    {header}")?;
        for line in content.as_ref().lines() {
            //                12345678
            writeln!(writer, "        {line}")?;
        }
        Ok(())
    }

    if output.status.success() {
        writeln!(&mut stdout, "[{host}] Completed {}", title(action))?;
    } else {
        writeln!(
            &mut stderr,
            "[{host}] Action failed. See below for details.",
        )?;
    }

    if !output.stdout.is_empty() {
        write_indented(
            stdout.deref_mut(),
            "Captured stdout:",
            String::from_utf8_lossy(&output.stdout),
        )?;
    }

    if !output.stderr.is_empty() {
        write_indented(
            stderr.deref_mut(),
            "Captured stderr:",
            String::from_utf8_lossy(&output.stderr),
        )?;
    }

    if !output.status.success() {
        let exit_code_message = match output.status.code() {
            Some(i) => format!("exit code {i}"),
            None => "error".to_string(),
        };
        let yaml = serde_yaml::to_string(action).unwrap();
        writeln!(
            &mut stderr,
            "Action exited with {exit_code_message}:\n{yaml}",
        )?;
    }
    Ok(())
}

/// A testable method containing the logic for reporting that an [Action] is starting.
pub fn _starting<OT: Write, O: DerefMut<Target = OT>>(
    mut stdout: O,
    host: &str,
    action: &Action,
) -> io::Result<()> {
    let action = title(action);
    writeln!(
        &mut stdout,
        // Adding one extra space lines up "Starting" with "Completed" in the final output.
        "[{host}] Starting  {action}",
        // Ex:    Completed {action}
    )
}

#[cfg(test)]
mod test;
