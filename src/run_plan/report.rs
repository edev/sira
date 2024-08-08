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
use std::collections::BTreeMap;
use std::fmt::Display;
use std::io::{self, Write};
use std::process::Output;
use std::time::Instant;
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
#[derive(Clone, Default, Debug)]
pub struct Reporter {
    durations: BTreeMap<String, Instant>,
}

impl Reporter {
    pub fn new() -> Self {
        Self {
            durations: BTreeMap::new(),
        }
    }
}

#[async_trait]
impl Report for Reporter {
    async fn starting(&mut self, host: &str, action: &Action) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        let _ = self.durations.insert(host.to_owned(), Instant::now());
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
        let duration = match self.durations.remove(host) {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => {
                // If this runs, then the caller called report for a host without first calling
                // start. That probably indicates a bug, but it's a very minor one. So, we'll try
                // to report the error, but we won't panic if there's a problem doing so.
                let _ = writeln!(
                    &mut stderr,
                    "BUG: could not retrieve action duration for host: {host}"
                );
                0.0
            }
        };
        task::block_in_place(move || {
            _report(&mut stdout, &mut stderr, host, action, output, duration)
        })
    }
}

/// Prints a message with a header indicating that it comes from or pertains to a specific host.
///
/// Any reporting on a host should likely incorporate at least one call to this function at the
/// start of printed output.
pub fn print_host_message<D: Write>(
    destination: &mut D,
    host: impl Display,
    message: impl Display,
) -> io::Result<()> {
    writeln!(destination, "[{host}] {message}")
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

/// A testable function containing the logic for reporting the outcome of an [Action].
pub(crate) fn _report<O: Write, E: Write>(
    stdout: &mut O,
    stderr: &mut E,
    host: &str,
    action: &Action,
    output: &Output,
    duration: f64,
) -> io::Result<()> {
    fn write_indented<W: Write>(
        writer: &mut W,
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
        print_host_message(
            stdout,
            host,
            format!("Completed {} ({duration:.3}s)", title(action)),
        )?;
    } else {
        print_host_message(
            stderr,
            host,
            "Action failed. See below for details. ({duration:.3}s)",
        )?;
    }

    if !output.stdout.is_empty() {
        write_indented(
            stdout,
            "Captured stdout:",
            String::from_utf8_lossy(&output.stdout),
        )?;
    }

    if !output.stderr.is_empty() {
        write_indented(
            stderr,
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
        writeln!(stderr, "Action exited with {exit_code_message}:\n{yaml}")?;
    }
    Ok(())
}

/// A testable function containing the logic for reporting that an [Action] is starting.
pub(crate) fn _starting<O: Write>(stdout: &mut O, host: &str, action: &Action) -> io::Result<()> {
    let action = title(action);

    // Adding one extra space lines up "Starting" with "Completed" in the final output:
    //                     Completed {action}
    let message = format!("Starting  {action}");
    print_host_message(stdout, host, message)
}

#[cfg(test)]
mod test;
