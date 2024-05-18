use anyhow::bail;
use sira::core::Plan;
use sira::run_plan;
use sira::run_plan::report;
use std::collections::BTreeMap;
use std::env;
use std::fmt::Display;
use std::io::{self, Write};

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

    let unsorted_errors = match run_plan(plan).await {
        Err(errors) => errors,
        Ok(()) => return Ok(()),
    };

    // Error values that resulted from connections problems; these will not trigger an error exit
    // from this program, but we will need to report them.
    //
    // Stored as a BTreeMap (host -> error) for alphabetical sorting by host.
    let mut connection_errors: BTreeMap<String, openssh::Error> = BTreeMap::new();

    // Any other error values; these will trigger an error exit from this program, and we also need
    // to report them.
    //
    // Stored as a BTreeMap (host -> error) for alphabetical sorting by host.
    let mut other_errors: BTreeMap<String, anyhow::Error> = BTreeMap::new();

    for (host, error) in unsorted_errors {
        use openssh::Error::*;

        // Try to downcast anyhow::Error to openssh::Error for further processing. If this fails,
        // dump the error in the general pile and continue.
        let error = match error.downcast::<openssh::Error>() {
            Ok(err) => err,
            Err(orig) => {
                safe_insert_other_error(&mut other_errors, &host, orig)?;
                continue;
            }
        };

        match error {
            err @ Master(_) | err @ Connect(_) | err @ Disconnected => {
                safe_insert_connection_error(&mut connection_errors, &host, err)?;
            }
            other => {
                safe_insert_other_error(&mut other_errors, &host, other.into())?;
            }
        }
    }

    // Print final reports.
    if !connection_errors.is_empty() {
        let mut stdout_lock = io::stdout().lock();
        writeln!(
            &mut stdout_lock,
            "\n\
            ==================\n\
            Connection issues:\n\
            ==================\n\
            \n\
            The following hosts encountered connection issues and could not complete their runs:\n",
        )?;
        for (host, error) in connection_errors {
            report::print_host_message(&mut stdout_lock, host, error)?;
        }
    }
    if !other_errors.is_empty() {
        let mut stderr_lock = io::stderr().lock();
        writeln!(
            &mut stderr_lock,
            "\n\
            =======\n\
            Errors:\n\
            =======\n\
            \n\
            The following hosts encountered errors and had to abort their their runs:\n",
        )?;
        for (host, error) in other_errors {
            report::print_host_message(&mut stderr_lock, host, error)?;
        }
        writeln!(&mut stderr_lock)?;
        bail!("Exiting with error due to the errors listed above.");
    }
    Ok(())
}

/// Inserts a value into `connection_errors` in `main`.
fn safe_insert_connection_error<H: Display>(
    map: &mut BTreeMap<String, openssh::Error>,
    host: &H,
    error: openssh::Error,
) -> std::io::Result<()> {
    _safe_insert(map, "connection_errors", host, error)
}

/// Inserts a value into `other_errors` in `main`.
fn safe_insert_other_error<H: Display>(
    map: &mut BTreeMap<String, anyhow::Error>,
    host: &H,
    error: anyhow::Error,
) -> std::io::Result<()> {
    _safe_insert(map, "other_errors", host, error)
}

/// Inserts a (host, error) pair into a BTreeMap, reporting a bug if a host appears twice.
///
/// For internal use only.
fn _safe_insert<E: Display, H: Display>(
    map: &mut BTreeMap<String, E>,
    map_name: &'static str,
    host: &H,
    error: E,
) -> std::io::Result<()> {
    let evicted = map.insert(host.to_string(), error);
    if let Some(evicted) = evicted {
        let mut stderr_lock = io::stderr().lock();
        writeln!(
            &mut stderr_lock,
            "BUG: there should be at most one error per host, but found multiple! \
            See below for details.\n\
            Evicted an error from {map_name}:"
        )?;
        report::print_host_message(&mut stderr_lock, host, evicted)?;
        writeln!(&mut stderr_lock, "Please report this bug!")?;
    }
    Ok(())
}
